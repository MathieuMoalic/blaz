use axum::{
    Json,
    extract::{Path, State},
};
use reqwest::StatusCode;
use serde::Serialize;

use crate::{
    error::AppResult,
    models::{AppState, NewCategory, ReorderCategories, ShoppingCategory, UpdateCategory},
};

/// GET /categories
/// List all shopping categories ordered by `sort_order`.
pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<ShoppingCategory>>> {
    let rows: Vec<ShoppingCategory> = sqlx::query_as(
        r"SELECT id, name, sort_order, created_at FROM shopping_categories ORDER BY sort_order",
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows))
}

/// POST /categories
/// Create a new shopping category.
pub async fn create(
    State(state): State<AppState>,
    Json(req): Json<NewCategory>,
) -> AppResult<Json<ShoppingCategory>> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Category name cannot be empty".to_string()).into());
    }

    // Get max sort_order to append at end
    let max_order: Option<i64> =
        sqlx::query_scalar(r"SELECT MAX(sort_order) FROM shopping_categories")
            .fetch_one(&state.pool)
            .await?;
    let new_order = max_order.unwrap_or(0) + 1;

    let result = sqlx::query(
        r"INSERT INTO shopping_categories (name, sort_order) VALUES (?, ?)",
    )
    .bind(name)
    .bind(new_order)
    .execute(&state.pool)
    .await;

    match result {
        Ok(_) => {}
        Err(e) => {
            if let sqlx::Error::Database(db) = &e
                && db.is_unique_violation()
            {
                return Err((
                    StatusCode::CONFLICT,
                    format!("Category '{name}' already exists"),
                )
                    .into());
            }
            return Err(e.into());
        }
    }

    // Fetch the created category
    let row: ShoppingCategory = sqlx::query_as(
        r"SELECT id, name, sort_order, created_at FROM shopping_categories WHERE name = ?",
    )
    .bind(name)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(row))
}

/// PATCH /categories/{id}
/// Update a category's name or `sort_order`.
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateCategory>,
) -> AppResult<Json<ShoppingCategory>> {
    // Verify category exists
    let existing: Option<ShoppingCategory> = sqlx::query_as(
        r"SELECT id, name, sort_order, created_at FROM shopping_categories WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;

    let Some(existing) = existing else {
        return Err(StatusCode::NOT_FOUND.into());
    };

    // Build dynamic update
    let mut updates = Vec::new();
    let mut binds: Vec<String> = Vec::new();

    if let Some(name) = &req.name {
        let name = name.trim();
        if name.is_empty() {
            return Err(
                (StatusCode::BAD_REQUEST, "Category name cannot be empty".to_string()).into(),
            );
        }
        updates.push("name = ?");
        binds.push(name.to_string());
    }

    if let Some(order) = req.sort_order {
        updates.push("sort_order = ?");
        binds.push(order.to_string());
    }

    if updates.is_empty() {
        return Ok(Json(existing));
    }

    let sql = format!(
        "UPDATE shopping_categories SET {} WHERE id = ?",
        updates.join(", ")
    );

    let mut query = sqlx::query(&sql);
    for b in &binds {
        query = query.bind(b);
    }
    query = query.bind(id);

    let result = query.execute(&state.pool).await;

    match result {
        Ok(_) => {}
        Err(e) => {
            if let sqlx::Error::Database(db) = &e
                && db.is_unique_violation()
            {
                return Err((StatusCode::CONFLICT, "Category name already exists".to_string())
                    .into());
            }
            return Err(e.into());
        }
    }

    // Fetch updated
    let row: ShoppingCategory = sqlx::query_as(
        r"SELECT id, name, sort_order, created_at FROM shopping_categories WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(row))
}

#[derive(Serialize)]
pub struct DeleteResponse {
    pub deleted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items_using: Option<i64>,
}

/// DELETE /categories/{id}
/// Delete a category. Fails if "Other" category or if items are using it.
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<DeleteResponse>> {
    // Fetch the category
    let existing: Option<ShoppingCategory> = sqlx::query_as(
        r"SELECT id, name, sort_order, created_at FROM shopping_categories WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;

    let Some(existing) = existing else {
        return Err(StatusCode::NOT_FOUND.into());
    };

    // Prevent deleting "Other" category
    if existing.name == "Other" {
        return Err((
            StatusCode::FORBIDDEN,
            "Cannot delete the 'Other' category".to_string(),
        )
            .into());
    }

    // Check if any shopping items use this category
    let count: i64 = sqlx::query_scalar(
        r"SELECT COUNT(*) FROM shopping_items WHERE category = ?",
    )
    .bind(&existing.name)
    .fetch_one(&state.pool)
    .await?;

    if count > 0 {
        return Err((
            StatusCode::CONFLICT,
            format!("{count} item(s) are using this category. Reassign them first."),
        )
            .into());
    }

    // Delete the category
    sqlx::query(r"DELETE FROM shopping_categories WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;

    Ok(Json(DeleteResponse {
        deleted: true,
        items_using: None,
    }))
}

/// POST /categories/reorder
/// Reorder categories by providing list of IDs in desired order.
pub async fn reorder(
    State(state): State<AppState>,
    Json(req): Json<ReorderCategories>,
) -> AppResult<Json<Vec<ShoppingCategory>>> {
    // Update sort_order for each category based on position in the list
    for (idx, id) in req.order.iter().enumerate() {
        #[allow(clippy::cast_possible_wrap)]
        let order = idx as i64;
        sqlx::query(r"UPDATE shopping_categories SET sort_order = ? WHERE id = ?")
            .bind(order)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }

    // Return updated list
    let rows: Vec<ShoppingCategory> = sqlx::query_as(
        r"SELECT id, name, sort_order, created_at FROM shopping_categories ORDER BY sort_order",
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows))
}
