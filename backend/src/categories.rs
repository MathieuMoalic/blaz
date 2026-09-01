use crate::models::AppState;

/// Check if a category name exists in the database.
///
/// # Errors
///
/// Returns `Ok(false)`-shaped results through `Option`; database failures
/// map to `Ok(None)` → treated as invalid by callers.
pub async fn validate_category(state: &AppState, name: &str) -> bool {
    sqlx::query_scalar::<_, i64>(r"SELECT 1 FROM shopping_categories WHERE name = ?")
        .bind(name)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten()
        .is_some()
}
