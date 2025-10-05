use axum::{
    Json,
    extract::{Path, State},
};

use crate::{
    error::AppResult,
    models::{AppState, NewItem, ShoppingItem, ToggleItem},
};

// GET /shopping
pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<ShoppingItem>>> {
    let rows =
        sqlx::query_as::<_, ShoppingItem>("SELECT id, text, done FROM shopping_items ORDER BY id")
            .fetch_all(&state.pool)
            .await?;
    Ok(Json(rows))
}

// POST /shopping { text }
pub async fn create(
    State(state): State<AppState>,
    Json(new): Json<NewItem>,
) -> AppResult<Json<ShoppingItem>> {
    let row = sqlx::query_as::<_, ShoppingItem>(
        "INSERT INTO shopping_items(text, done) VALUES (?, 0)
         RETURNING id, text, done",
    )
    .bind(new.text)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

// PATCH /shopping/:id { done: bool }
pub async fn toggle_done(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(t): Json<ToggleItem>,
) -> AppResult<Json<ShoppingItem>> {
    let done = if t.done { 1i64 } else { 0i64 };
    let row = sqlx::query_as::<_, ShoppingItem>(
        "UPDATE shopping_items SET done = ?
         WHERE id = ?
         RETURNING id, text, done",
    )
    .bind(done)
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(row))
}

// DELETE /shopping/:id
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let affected = sqlx::query("DELETE FROM shopping_items WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?
        .rows_affected();

    Ok(Json(serde_json::json!({ "deleted": affected })))
}
