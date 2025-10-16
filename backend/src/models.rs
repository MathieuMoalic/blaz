use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use sqlx::{FromRow, SqlitePool};
use std::path::PathBuf;

use crate::ingredient_parser::parse_ingredient_line;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub media_dir: PathBuf,
    pub jwt_encoding: jsonwebtoken::EncodingKey,
    pub jwt_decoding: jsonwebtoken::DecodingKey,
    pub allow_registration: bool,
}

/* ---------- API models ---------- */

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Ingredient {
    pub quantity: Option<f64>, // e.g. 120.0
    pub unit: Option<String>,  // "g","kg","ml","L","tsp","tbsp" (normalized)
    pub name: String,          // "flour"
}

/* Accept either a string OR the structured object when reading existing rows */
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum IngredientRepr {
    S(String),
    O(Ingredient),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Recipe {
    pub id: i64,
    pub title: String,
    pub source: String,
    #[serde(rename = "yield")]
    pub r#yield: String,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
    pub ingredients: Vec<Ingredient>,
    pub instructions: Vec<String>,
    pub image_path: Option<String>,
    pub image_path_small: Option<String>,
    pub image_path_full: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct NewRecipe {
    pub title: String,
    #[serde(default)]
    pub source: String,
    #[serde(default, rename = "yield")]
    pub r#yield: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub ingredients: Vec<String>,
    #[serde(default)]
    pub instructions: Vec<String>,
}

#[derive(Deserialize, Debug, Default)]
pub struct UpdateRecipe {
    pub title: Option<String>,
    pub source: Option<String>,
    #[serde(rename = "yield")]
    pub r#yield: Option<String>,
    pub notes: Option<String>,
    pub ingredients: Option<Vec<String>>,
    pub instructions: Option<Vec<String>>,
}

/* ---------- DB row model ---------- */

#[derive(FromRow)]
pub struct RecipeRow {
    pub id: i64,
    pub title: String,
    pub source: String,
    pub r#yield: String,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
    // IMPORTANT: let rows load even if they still have ["2 carrots", ...]
    pub ingredients: Json<Vec<IngredientRepr>>,
    pub instructions: Json<Vec<String>>,
    pub image_path: Option<String>,
    pub image_path_small: Option<String>,
    pub image_path_full: Option<String>,
}

impl From<RecipeRow> for Recipe {
    fn from(r: RecipeRow) -> Self {
        // normalize DB payload (strings or structured) into structured vector
        let ingredients = r
            .ingredients
            .0
            .into_iter()
            .map(|x| match x {
                IngredientRepr::O(o) => o,
                IngredientRepr::S(s) => parse_ingredient_line(&s),
            })
            .collect();

        Self {
            id: r.id,
            title: r.title,
            source: r.source,
            r#yield: r.r#yield,
            notes: r.notes,
            created_at: r.created_at,
            updated_at: r.updated_at,
            ingredients,
            instructions: r.instructions.0,
            image_path: r.image_path,
            image_path_full: r.image_path_full,
            image_path_small: r.image_path_small,
        }
    }
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
