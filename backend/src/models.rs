use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
}

/* ---------- Recipes ---------- */

#[derive(Serialize, Deserialize, FromRow, Clone)]
pub struct Recipe {
    pub id: i64,
    pub title: String,
}

#[derive(Deserialize)]
pub struct NewRecipe {
    pub title: String,
}

/* ---------- Meal plan ---------- */

#[derive(Serialize, Deserialize, FromRow, Clone)]
pub struct MealPlanEntry {
    pub id: i64,
    pub day: String, // "YYYY-MM-DD"
    pub recipe_id: i64,
    pub title: String, // joined from recipes for convenience
}

#[derive(Deserialize)]
pub struct AssignRecipe {
    pub day: String, // "YYYY-MM-DD"
    pub recipe_id: i64,
}

/* ---------- Shopping list ---------- */

#[derive(Serialize, Deserialize, FromRow, Clone)]
pub struct ShoppingItem {
    pub id: i64,
    pub text: String,
    pub done: i64, // 0/1
}

#[derive(Deserialize)]
pub struct NewItem {
    pub text: String,
}

#[derive(Deserialize)]
pub struct ToggleItem {
    pub done: bool,
}
