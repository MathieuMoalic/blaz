use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use sqlx::Row;

use crate::{
    error::AppResult,
    models::{AppState, AssignRecipe, MealPlanEntry},
};

#[derive(Deserialize)]
pub struct DayQuery {
    pub day: String,
} // YYYY-MM-DD

// GET /meal-plan?day=YYYY-MM-DD
pub async fn get_for_day(
    State(state): State<AppState>,
    Query(q): Query<DayQuery>,
) -> AppResult<Json<Vec<MealPlanEntry>>> {
    let rows = sqlx::query_as::<_, MealPlanEntry>(
        r#"
        SELECT mp.id, mp.day, mp.recipe_id, r.title
        FROM meal_plan mp
        JOIN recipes r ON r.id = mp.recipe_id
        WHERE mp.day = ?
        ORDER BY mp.id
        "#,
    )
    .bind(q.day)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows))
}

// POST /meal-plan { day, recipe_id }
pub async fn assign(
    State(state): State<AppState>,
    Json(payload): Json<AssignRecipe>,
) -> AppResult<Json<MealPlanEntry>> {
    let mut tx = state.pool.begin().await?;

    // 1) Insert
    sqlx::query(r#"INSERT INTO meal_plan(day, recipe_id) VALUES (?, ?)"#)
        .bind(&payload.day)
        .bind(payload.recipe_id)
        .execute(&mut *tx)
        .await?;

    // 2) Get new id on the SAME connection
    let row = sqlx::query(r#"SELECT last_insert_rowid() AS id"#)
        .fetch_one(&mut *tx)
        .await?;
    let new_id: i64 = row.get::<i64, _>("id");

    // 3) Read the joined row to include title
    let entry = sqlx::query_as::<_, MealPlanEntry>(
        r#"
        SELECT mp.id, mp.day, mp.recipe_id, r.title
        FROM meal_plan mp
        JOIN recipes r ON r.id = mp.recipe_id
        WHERE mp.id = ?
        "#,
    )
    .bind(new_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(entry))
}

// DELETE /meal-plan/{day}/{recipe_id}
pub async fn unassign(
    State(state): State<AppState>,
    Path((day, recipe_id)): Path<(String, i64)>,
) -> AppResult<Json<serde_json::Value>> {
    let affected = sqlx::query("DELETE FROM meal_plan WHERE day = ? AND recipe_id = ?")
        .bind(day)
        .bind(recipe_id)
        .execute(&state.pool)
        .await?
        .rows_affected();

    Ok(Json(serde_json::json!({ "deleted": affected })))
}
