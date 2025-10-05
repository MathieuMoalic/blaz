use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use sqlx::types::Json as SqlxJson;
use sqlx::{QueryBuilder, Sqlite};

use crate::models::{AppState, NewRecipe, Recipe, RecipeRow, UpdateRecipe};

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Recipe>>, StatusCode> {
    let rows: Vec<RecipeRow> = sqlx::query_as::<_, RecipeRow>(
        r#"
        SELECT id, title, source, "yield", notes,
               created_at, updated_at,
               ingredients, instructions
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
               ingredients, instructions
        FROM recipes WHERE id = ?
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

    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(
        r#"
        INSERT INTO recipes (title, source, "yield", notes, ingredients, instructions, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        RETURNING id, title, source, "yield", notes,
                  created_at, updated_at,
                  ingredients, instructions
        "#,
    )
    .bind(new.title)
    .bind(new.source)
    .bind(new.r#yield)
    .bind(new.notes)
    .bind(SqlxJson(new.ingredients))     // stored as TEXT JSON
    .bind(SqlxJson(new.instructions))
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
    // Build dynamic UPDATE … SET …
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new("UPDATE recipes SET ");
    let mut sep = qb.separated(", ");

    if let Some(title) = up.title {
        sep.push("title = ").push_bind(title);
    }
    if let Some(source) = up.source {
        sep.push("source = ").push_bind(source);
    }
    if let Some(y) = up.r#yield {
        sep.push(r#""yield" = "#).push_bind(y);
    }
    if let Some(notes) = up.notes {
        sep.push("notes = ").push_bind(notes);
    }
    if let Some(ing) = up.ingredients {
        sep.push("ingredients = ").push_bind(SqlxJson(ing));
    }
    if let Some(instr) = up.instructions {
        sep.push("instructions = ").push_bind(SqlxJson(instr));
    }

    // Always touch updated_at
    sep.push("updated_at = CURRENT_TIMESTAMP");

    qb.push(" WHERE id = ").push_bind(id);

    let res = qb
        .build()
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if res.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    // Return the fresh row
    get(State(state), Path(id)).await
}
