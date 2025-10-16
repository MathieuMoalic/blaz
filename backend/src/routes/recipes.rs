use crate::image_io::to_full_and_thumb_webp;
use crate::{
    ingredient_parser::parse_ingredient_line,
    models::{AppState, Ingredient, NewRecipe, Recipe, RecipeRow, UpdateRecipe},
};
use axum::{
    Json,
    extract::{Multipart, Path, State},
    http::StatusCode,
};
use sqlx::Arguments;
use sqlx::sqlite::SqliteArguments;
use tracing::error;

use crate::error::AppResult;

pub async fn fetch_and_store_recipe_image(
    client: &reqwest::Client,
    abs_url: &str,
    state: &crate::models::AppState,
    recipe_id: i64,
) -> anyhow::Result<(String, String)> {
    use std::io;

    // 1) download
    let bytes = client
        .get(abs_url)
        .header(reqwest::header::USER_AGENT, "blaz/recipe-importer")
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec();

    // 2) decode + encode off-thread
    let (full_webp, thumb_webp) =
        tokio::task::spawn_blocking(move || -> io::Result<(Vec<u8>, Vec<u8>)> {
            let img = image::load_from_memory(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("decode error: {e}")))?;

            to_full_and_thumb_webp(&img)
        })
        .await??;

    // 3) write files
    tokio::fs::create_dir_all(&state.media_dir).await?;
    let uid = uuid::Uuid::new_v4();
    let full_name = format!("recipe_{recipe_id}_{uid}.webp");
    let thumb_name = format!("recipe_{recipe_id}_{uid}_sm.webp");
    tokio::fs::write(state.media_dir.join(&full_name), &full_webp).await?;
    tokio::fs::write(state.media_dir.join(&thumb_name), &thumb_webp).await?;

    Ok((full_name, thumb_name))
}

pub async fn upload_image(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    mut multipart: Multipart,
) -> AppResult<Json<Recipe>> {
    use uuid::Uuid;

    // 1) Pull the file bytes (accept "image" or "file")
    let mut bytes: Option<Vec<u8>> = None;
    while let Some(field) = multipart.next_field().await? {
        match field.name() {
            Some("image") | Some("file") => {
                bytes = Some(field.bytes().await?.to_vec());
                break;
            }
            _ => continue,
        }
    }

    // If nothing uploaded, just return the current recipe state
    let bytes = match bytes {
        Some(b) => b,
        None => {
            let recipe: Recipe = sqlx::query_as::<_, RecipeRow>(
                r#"
                SELECT id, title, source, "yield", notes,
                       created_at, updated_at,
                       ingredients, instructions,
                       image_path, image_path_small, image_path_full
                  FROM recipes
                 WHERE id = ?
                "#,
            )
            .bind(id)
            .fetch_one(&state.pool)
            .await?
            .into();
            return Ok(Json(recipe));
        }
    };

    // 2) Ensure media dir
    tokio::fs::create_dir_all(&state.media_dir).await?;

    // 3) Build filenames (always .webp)
    let uid = Uuid::new_v4();
    let full_name = format!("recipe_{id}_{uid}.webp");
    let thumb_name = format!("recipe_{id}_{uid}_sm.webp");
    let full_abs = state.media_dir.join(&full_name);
    let thumb_abs = state.media_dir.join(&thumb_name);

    // 4) Heavy work off the async thread: decode, resize, encode to WebP
    let (full_webp, thumb_webp): (Vec<u8>, Vec<u8>) =
        tokio::task::spawn_blocking(move || -> std::io::Result<(Vec<u8>, Vec<u8>)> {
            // Decode any common image format
            let img = image::load_from_memory(&bytes).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, format!("decode error: {e}"))
            })?;

            to_full_and_thumb_webp(&img)
        })
        .await??;

    // 5) Write both files
    tokio::fs::write(&full_abs, &full_webp).await?;
    tokio::fs::write(&thumb_abs, &thumb_webp).await?;

    // 6) Update DB (store relative filenames)
    sqlx::query(
        r#"
        UPDATE recipes
           SET image_path_full  = ?,
               image_path_small = ?,
               updated_at       = CURRENT_TIMESTAMP
         WHERE id = ?
        "#,
    )
    .bind(&full_name)
    .bind(&thumb_name)
    .bind(id)
    .execute(&state.pool)
    .await?;

    // 7) Return updated recipe
    let recipe: Recipe = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions,
               image_path, image_path_small, image_path_full
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

pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<Recipe>>> {
    // CHANGED
    let rows: Vec<RecipeRow> = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions,
               image_path, image_path_small, image_path_full
          FROM recipes
         ORDER BY id
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(rows.into_iter().map(Recipe::from).collect()))
}

pub async fn get(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Recipe>> {
    // CHANGED
    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions,
               image_path, image_path_small, image_path_full
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
) -> AppResult<Json<Recipe>> {
    // CHANGED
    if new.title.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST.into());
    }
    let structured: Vec<Ingredient> = new
        .ingredients
        .iter()
        .map(|s| parse_ingredient_line(s))
        .collect();
    let ingredients_json = serde_json::to_string(&structured).unwrap_or_else(|_| "[]".into());
    let instructions_json =
        serde_json::to_string(&new.instructions).unwrap_or_else(|_| "[]".into());

    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(r#"
    INSERT INTO recipes (title, source, "yield", notes, ingredients, instructions, created_at, updated_at)
    VALUES (?, ?, ?, ?, json(?), json(?), CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
    RETURNING id, title, source, "yield", notes,
              created_at, updated_at,
              ingredients, instructions,
              image_path, image_path_small, image_path_full"#)
    .bind(new.title)
    .bind(new.source)
    .bind(new.r#yield)
    .bind(new.notes)
    .bind(ingredients_json)    .bind(instructions_json)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(row.into()))
}

pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    // CHANGED
    let res = sqlx::query(r#"DELETE FROM recipes WHERE id = ?"#)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if res.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND.into());
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(up): Json<UpdateRecipe>,
) -> AppResult<Json<Recipe>> {
    // CHANGED
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

    if let Some(ing_lines) = up.ingredients.as_ref() {
        let structured: Vec<Ingredient> =
            ing_lines.iter().map(|s| parse_ingredient_line(s)).collect();

        let s = serde_json::to_string(&structured).unwrap_or_else(|_| "[]".into());
        sets.push("ingredients = json(?)");
        args.add(s).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    if let Some(instr) = up.instructions {
        let s = serde_json::to_string(&instr).unwrap_or_else(|_| "[]".to_string());
        sets.push("instructions = json(?)");
        args.add(s).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    sets.push("updated_at = CURRENT_TIMESTAMP");

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
        return Err(StatusCode::NOT_FOUND.into());
    }

    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions,
               image_path, image_path_small, image_path_full
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
