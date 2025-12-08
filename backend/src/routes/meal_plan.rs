use axum::{
    Json,
    extract::{Path, Query, State},
};
use reqwest::StatusCode;
use serde::Deserialize;

use crate::{
    error::AppResult,
    models::{AppState, AssignRecipe, MealPlanEntry},
};

#[derive(Deserialize)]
pub struct DayQuery {
    pub day: String, // "YYYY-MM-DD"
}

/// GET /meal-plan?day=YYYY-MM-DD
/// Get meal plan entries for a specific day.
///
/// # Errors
/// Returns an error if querying the meal plan entries for the given day fails.
pub async fn get_for_day(
    State(state): State<AppState>,
    Query(q): Query<DayQuery>,
) -> AppResult<Json<Vec<MealPlanEntry>>> {
    // Return entries for the day; join recipes to reflect latest title.
    let rows: Vec<MealPlanEntry> = sqlx::query_as::<_, MealPlanEntry>(
        r"
        SELECT mp.id,
               mp.day,
               mp.recipe_id,
               r.title AS title
          FROM meal_plan mp
          JOIN recipes r ON r.id = mp.recipe_id
         WHERE mp.day = ?
         ORDER BY mp.id
        ",
    )
    .bind(&q.day)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows))
}

/// POST /meal-plan  { "day": "YYYY-MM-DD", "`recipe_id"`: 123 }
/// Assign a recipe to a specific day in the meal plan.
///
/// # Errors
/// Returns an error if:
/// - The recipe title cannot be fetched (e.g., recipe does not exist).
/// - Inserting the meal plan entry fails.
pub async fn assign(
    State(state): State<AppState>,
    Json(req): Json<AssignRecipe>,
) -> AppResult<Json<MealPlanEntry>> {
    // 1) Fetch the current recipe title
    let (title,): (String,) = sqlx::query_as(r"SELECT title FROM recipes WHERE id = ?")
        .bind(req.recipe_id)
        .fetch_one(&state.pool)
        .await?;

    // 2) Insert into meal_plan including the title (NOT NULL)
    let resp = sqlx::query_as::<_, MealPlanEntry>(
        r"
        INSERT INTO meal_plan (day, recipe_id, title)
        VALUES (?, ?, ?)
        RETURNING id, day, recipe_id, title
        ",
    )
    .bind(&req.day)
    .bind(req.recipe_id)
    .bind(&title)
    .fetch_one(&state.pool)
    .await;

    let row = match resp {
        Ok(row) => row,
        Err(e) => {
            if let sqlx::Error::Database(db) = &e {
                if db.is_unique_violation() {
                    return Err(StatusCode::CONFLICT.into());
                }
            }
            return Err(e.into());
        }
    };

    Ok(Json(row))
}

/// DELETE /meal-plan/{day}/{recipe_id}
/// Unassign a recipe from a specific day in the meal plan.
///
/// # Errors
/// Returns an error if deleting the meal plan entry fails.
pub async fn unassign(
    State(state): State<AppState>,
    Path((day, recipe_id)): Path<(String, i64)>,
) -> AppResult<Json<serde_json::Value>> {
    let res = sqlx::query(r"DELETE FROM meal_plan WHERE day = ? AND recipe_id = ?")
        .bind(day)
        .bind(recipe_id)
        .execute(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({
        "deleted": res.rows_affected()
    })))
}
