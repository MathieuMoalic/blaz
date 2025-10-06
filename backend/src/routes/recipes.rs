use crate::models::{AppState, NewRecipe, Recipe, RecipeRow, UpdateRecipe};
use axum::{
    Json,
    extract::{Multipart, Path, State},
    http::StatusCode,
};
use sqlx::Arguments;
use sqlx::sqlite::SqliteArguments;
use tokio::io::AsyncWriteExt;
use tracing::error;

use crate::error::AppResult;

pub async fn upload_image(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    mut multipart: Multipart,
) -> AppResult<Json<Recipe>> {
    use uuid::Uuid;

    while let Some(field) = multipart.next_field().await? {
        if field.name() != Some("image") {
            continue;
        }

        let filename_hint = field.file_name().unwrap_or("upload.bin").to_string();
        let ext = filename_hint
            .rsplit('.')
            .next()
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_else(|| "bin".to_string());

        let fname = format!("{}.{ext}", Uuid::new_v4());
        let dest_rel = fname.clone();
        let dest_abs = state.media_dir.join(&fname);

        tokio::fs::create_dir_all(&state.media_dir).await?;
        let mut file = tokio::fs::File::create(&dest_abs).await?;
        let bytes = field.bytes().await?;
        file.write_all(&bytes).await?;
        file.flush().await?;

        sqlx::query(
            r#"UPDATE recipes
                 SET image_path = ?, updated_at = CURRENT_TIMESTAMP
               WHERE id = ?"#,
        )
        .bind(&dest_rel)
        .bind(id)
        .execute(&state.pool)
        .await?;

        break;
    }

    // Return updated recipe (now includes image_path)
    let recipe: Recipe = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions, image_path
          FROM recipes
         WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?
    .into();

    Ok(Json(recipe))
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Recipe>>, StatusCode> {
    let rows: Vec<RecipeRow> = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions, image_path
          FROM recipes
         ORDER BY id
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(rows.into_iter().map(Recipe::from).collect()))
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Recipe>, StatusCode> {
    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions, image_path
          FROM recipes
         WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::NOT_FOUND)?;

    Ok(Json(row.into()))
}

pub async fn create(
    State(state): State<AppState>,
    Json(new): Json<NewRecipe>,
) -> Result<Json<Recipe>, StatusCode> {
    if new.title.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    // RETURNING now includes image_path
    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(
        r#"
        INSERT INTO recipes (title, source, "yield", notes, ingredients, instructions, created_at, updated_at)
        VALUES (?, ?, ?, ?, json(?), json(?), CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        RETURNING id, title, source, "yield", notes,
                  created_at, updated_at,
                  ingredients, instructions, image_path
        "#,
    )
    .bind(new.title)
    .bind(new.source)
    .bind(new.r#yield)
    .bind(new.notes)
    .bind(serde_json::to_string(&new.ingredients).unwrap_or("[]".to_string()))
    .bind(serde_json::to_string(&new.instructions).unwrap_or("[]".to_string()))
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(row.into()))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let res = sqlx::query(r#"DELETE FROM recipes WHERE id = ?"#)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if res.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(up): Json<UpdateRecipe>,
) -> Result<Json<Recipe>, StatusCode> {
    let mut sets: Vec<&'static str> = Vec::new();
    let mut args = SqliteArguments::default();

    if let Some(title) = up.title {
        sets.push("title = ?");
        args.add(title)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    if let Some(source) = up.source {
        sets.push("source = ?");
        args.add(source)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    if let Some(y) = up.r#yield {
        sets.push(r#""yield" = ?"#);
        args.add(y).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    if let Some(notes) = up.notes {
        sets.push("notes = ?");
        args.add(notes)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    if let Some(ing) = up.ingredients {
        let s = serde_json::to_string(&ing).unwrap_or_else(|_| "[]".to_string());
        sets.push("ingredients = json(?)");
        args.add(s).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    if let Some(instr) = up.instructions {
        let s = serde_json::to_string(&instr).unwrap_or_else(|_| "[]".to_string());
        sets.push("instructions = json(?)");
        args.add(s).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    // Always touch updated_at
    sets.push("updated_at = CURRENT_TIMESTAMP");

    // WHERE clause
    let sql = format!("UPDATE recipes SET {} WHERE id = ?", sets.join(", "));
    args.add(id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let res = sqlx::query_with(&sql, args)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            error!(?e, "recipes.update failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if res.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    // Return fresh row
    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions, image_path
          FROM recipes
         WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        error!(?e, "recipes.get after update failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(row.into()))
}
