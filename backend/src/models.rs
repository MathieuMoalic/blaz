use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use sqlx::{FromRow, SqlitePool};

use crate::config::Config;

/* ---------- App state ---------- */
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub jwt_encoding: jsonwebtoken::EncodingKey,
    pub config: Config,
}

/* ---------- API models ---------- */

#[derive(Serialize, Deserialize, Clone, Debug)]
#[allow(clippy::struct_field_names)]
pub struct Ingredient {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>, // if Some, this item is a section header
    #[serde(default)]
    pub quantity: Option<f64>, // e.g. 120.0
    #[serde(default)]
    pub unit: Option<String>, // "g","kg","ml","L","tsp","tbsp" (normalized)
    #[serde(default)]
    pub name: String, // recipe-visible wording, e.g. "large potatoes"
    #[serde(default)]
    pub prep: Option<String>,
    /// Stable instance ID for this line within this recipe (not Food identity).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ingredient_id: Option<String>,
    /// Original line before parsing, where available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_text: Option<String>,
    /// Stable canonical food identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub food_id: Option<i64>,
    /// Semantic qualifiers that don't define food identity (e.g. "large").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub qualifiers: Vec<String>,
    /// How `food_id` was resolved
    /// (`confirmed_alias`/`alias`/`food`/`deterministic`/`llm`/`new_food`/`user`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_confidence: Option<f64>,
    /// `true` when semantic identity needs user review.
    #[serde(default)]
    pub needs_review: bool,
    /// `true` = raw unparsed text; `false` = user-confirmed structured ingredient.
    #[serde(default)]
    pub raw: bool,
    /// Canonical ingredient name for merging shopping list items.
    /// Deprecated: kept for display/debug during the `food_id` transition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_name: Option<String>,
}

impl Ingredient {
    /// A section header row.
    #[must_use]
    pub const fn section_header(label: String) -> Self {
        Self {
            section: Some(label),
            quantity: None,
            unit: None,
            name: String::new(),
            prep: None,
            ingredient_id: None,
            raw_text: None,
            food_id: None,
            qualifiers: Vec::new(),
            resolution_source: None,
            resolution_confidence: None,
            needs_review: false,
            raw: false,
            canonical_name: None,
        }
    }

    /// A structured ingredient from the deterministic parser.
    #[must_use]
    #[allow(dead_code)] // consumed from commit 6 (import pipeline) onwards
    pub fn from_parsed(parsed: &crate::ingredients::parser::ParsedIngredient) -> Self {
        Self {
            section: None,
            quantity: parsed.quantity,
            unit: parsed.unit.map(str::to_string),
            name: parsed.ingredient_phrase.clone(),
            prep: parsed.prep.clone(),
            ingredient_id: Some(uuid::Uuid::new_v4().to_string()),
            raw_text: Some(parsed.raw_text.clone()),
            food_id: None,
            qualifiers: Vec::new(),
            resolution_source: None,
            resolution_confidence: None,
            needs_review: false,
            raw: false,
            canonical_name: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IngredientMacros {
    pub name: String,
    pub protein_g: f64,
    pub fat_g: f64,
    pub carbs_g: f64,
    pub skipped: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RecipeMacros {
    /// `per_serving` if yield could be parsed as N servings, otherwise `per_recipe`.
    pub basis: String,
    pub protein_g: f64,
    pub fat_g: f64,   // saturated + unsaturated combined
    pub carbs_g: f64, // excluding fiber
    #[serde(default)]
    pub ingredients: Vec<IngredientMacros>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PrepReminder {
    pub step: String,
    pub hours_before: i32,
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
    pub image_path_small: Option<String>,
    pub image_path_full: Option<String>,
    pub macros: Option<RecipeMacros>,
    pub share_token: Option<String>,
    pub prep_reminders: Option<Vec<PrepReminder>>,
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
    pub ingredients: Vec<Ingredient>,
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
    pub ingredients: Option<Vec<Ingredient>>,
    pub instructions: Option<Vec<String>>,
    pub prep_reminders: Option<Vec<PrepReminder>>,
}

/* ---------- DB row model ---------- */

#[derive(FromRow)]
pub struct RecipeRow {
    pub id: i64,
    pub title: String,
    pub source: String,
    #[sqlx(rename = "yield")] // ensure mapping from column "yield"
    pub r#yield: String,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
    // IMPORTANT: let rows load even if they still have ["2 carrots", ...]
    pub ingredients: Json<Vec<Ingredient>>,
    pub instructions: Json<Vec<String>>,
    pub image_path_small: Option<String>,
    pub image_path_full: Option<String>,
    pub macros: Option<Json<RecipeMacros>>,
    pub share_token: Option<String>,
    pub prep_reminders: Option<Json<Vec<PrepReminder>>>,
}

impl From<RecipeRow> for Recipe {
    fn from(r: RecipeRow) -> Self {
        Self {
            id: r.id,
            title: r.title,
            source: r.source,
            r#yield: r.r#yield,
            notes: r.notes,
            created_at: r.created_at,
            updated_at: r.updated_at,
            ingredients: r.ingredients.0,
            instructions: r.instructions.0,
            image_path_full: r.image_path_full,
            image_path_small: r.image_path_small,
            macros: r.macros.map(|j| j.0),
            share_token: r.share_token,
            prep_reminders: r.prep_reminders.map(|j| j.0),
        }
    }
}

/* ---------- Meal plan ---------- */

#[derive(Serialize, Deserialize, FromRow, Clone)]
pub struct MealPlanEntry {
    pub id: i64,
    pub day: String, // "YYYY-MM-DD"
    pub recipe_id: i64,
    pub title: String,                    // joined from recipes for convenience
    pub image_path_small: Option<String>, // joined from recipes
}

#[derive(Deserialize)]
pub struct AssignRecipe {
    pub day: String, // "YYYY-MM-DD"
    pub recipe_id: i64,
}

/* ---------- Shopping list ---------- */

#[derive(Serialize, sqlx::FromRow, Clone)]
pub struct ShoppingItemView {
    pub id: i64,
    pub text: String,
    pub done: i64,
    pub category: Option<String>,
    pub notes: String,
    pub recipe_ids: String,            // JSON array like "[1,2,3]"
    pub recipe_titles: Option<String>, // Comma-separated like "Recipe A, Recipe B"
}

#[derive(Deserialize)]
pub struct NewItem {
    pub text: String,
}

/* ---------- Shopping categories ---------- */

#[derive(Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct ShoppingCategory {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct NewCategory {
    pub name: String,
}

#[derive(Deserialize, Default)]
pub struct UpdateCategory {
    pub name: Option<String>,
    pub sort_order: Option<i64>,
}

#[derive(Deserialize)]
pub struct ReorderCategories {
    pub order: Vec<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_ingredient_json_still_loads() {
        let legacy = serde_json::json!({
            "quantity": 2.0,
            "unit": "g",
            "name": "spaghetti",
            "raw": false
        });
        let ing: Ingredient = serde_json::from_value(legacy).expect("legacy loads");
        assert_eq!(ing.name, "spaghetti");
        assert_eq!(ing.quantity, Some(2.0));
        assert_eq!(ing.food_id, None);
        assert!(ing.qualifiers.is_empty());
        assert!(!ing.needs_review);
        assert_eq!(ing.ingredient_id, None);
        assert_eq!(ing.raw_text, None);
    }

    #[test]
    fn bare_string_array_ingredient_json_still_loads() {
        let legacy = serde_json::json!([{ "name": "2 carrots" }]);
        let ings: Vec<Ingredient> = serde_json::from_value(legacy).expect("legacy array loads");
        assert_eq!(ings.len(), 1);
        assert_eq!(ings[0].name, "2 carrots");
    }

    #[test]
    fn new_ingredient_fields_round_trip() {
        let ing = Ingredient {
            section: None,
            quantity: Some(3.0),
            unit: None,
            name: "large potatoes".to_string(),
            prep: Some("peeled".to_string()),
            ingredient_id: Some("uuid-1".to_string()),
            raw_text: Some("3 large potatoes, peeled".to_string()),
            food_id: Some(42),
            qualifiers: vec!["large".to_string()],
            resolution_source: Some("confirmed_alias".to_string()),
            resolution_confidence: Some(1.0),
            needs_review: false,
            raw: false,
            canonical_name: None,
        };
        let value = serde_json::to_value(&ing).expect("serialize");
        let back: Ingredient = serde_json::from_value(value).expect("deserialize");
        assert_eq!(back.food_id, Some(42));
        assert_eq!(back.ingredient_id.as_deref(), Some("uuid-1"));
        assert_eq!(back.raw_text.as_deref(), Some("3 large potatoes, peeled"));
        assert_eq!(back.qualifiers, vec!["large".to_string()]);
        assert_eq!(back.resolution_source.as_deref(), Some("confirmed_alias"));
        assert!(!back.needs_review);
    }

    #[test]
    fn from_parsed_carries_raw_text_and_id() {
        let parsed = crate::ingredients::parser::parse_ingredient_line("3 large potatoes, peeled")
            .expect("parses");
        let ing = Ingredient::from_parsed(&parsed);
        assert_eq!(ing.name, "large potatoes");
        assert_eq!(ing.prep.as_deref(), Some("peeled"));
        assert_eq!(ing.raw_text.as_deref(), Some("3 large potatoes, peeled"));
        assert!(ing.ingredient_id.is_some());
        assert!(!ing.raw);
    }
}
