use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use uuid::Uuid;

use crate::error::AppResult;
use crate::models::{AppState, Recipe, RecipeRow};
use crate::routes::recipes::RECIPE_COLS;

/// `POST /recipes/:id/share` — generate (or return existing) share token.
///
/// # Errors
/// Returns 404 if recipe not found, 500 on DB error.
pub async fn create_share_token(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let existing: Option<Option<String>> = sqlx::query_scalar(
        "SELECT share_token FROM recipes WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;

    match existing {
        Some(Some(token)) => return Ok(Json(serde_json::json!({ "share_token": token }))),
        None => return Err((StatusCode::NOT_FOUND, "Recipe not found".to_string()).into()),
        Some(None) => {}
    }

    let token = Uuid::new_v4().to_string();
    sqlx::query("UPDATE recipes SET share_token = ? WHERE id = ?")
        .bind(&token)
        .bind(id)
        .execute(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({ "share_token": token })))
}

/// `DELETE /recipes/:id/share` — revoke share token.
///
/// # Errors
/// Returns 404 if recipe not found, 500 on DB error.
pub async fn revoke_share_token(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let rows = sqlx::query("UPDATE recipes SET share_token = NULL WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?
        .rows_affected();

    if rows == 0 {
        Err((StatusCode::NOT_FOUND, "Recipe not found".to_string()).into())
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

/// `GET /share/:token` — public, no auth required.
///
/// # Errors
/// Returns 404 if token unknown, 500 on DB error.
pub async fn get_shared_recipe(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> AppResult<Json<Recipe>> {
    let sql = format!("SELECT {RECIPE_COLS} FROM recipes WHERE share_token = ?");
    let recipe: Option<Recipe> = sqlx::query_as::<_, RecipeRow>(&sql)
        .bind(&token)
        .fetch_optional(&state.pool)
        .await?
        .map(Into::into);

    recipe
        .map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Share link not found".to_string()).into())
}
