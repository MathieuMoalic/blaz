use axum::{
    Json,
    extract::{Path, State},
};

use crate::{
    error::AppResult,
    models::{AppState, NewRecipe, Recipe},
};

pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<Recipe>>> {
    let rows = sqlx::query_as::<_, Recipe>("SELECT id, title FROM recipes ORDER BY id")
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(rows))
}

pub async fn get(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Recipe>> {
    let row = sqlx::query_as::<_, Recipe>("SELECT id, title FROM recipes WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(row))
}

pub async fn create(
    State(state): State<AppState>,
    Json(new): Json<NewRecipe>,
) -> AppResult<Json<Recipe>> {
    let row =
        sqlx::query_as::<_, Recipe>("INSERT INTO recipes(title) VALUES (?) RETURNING id, title")
            .bind(new.title)
            .fetch_one(&state.pool)
            .await?;
    Ok(Json(row))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let affected = sqlx::query("DELETE FROM recipes WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?
        .rows_affected();

    Ok(Json(serde_json::json!({ "deleted": affected })))
}
