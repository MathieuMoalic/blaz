use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;

use crate::models::{AppState, Ingredient};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct JsonLdRecipe {
    #[serde(default, deserialize_with = "string_or_array")]
    name: Option<String>,
    #[serde(default, deserialize_with = "string_or_array")]
    description: Option<String>,
    #[serde(default, deserialize_with = "string_or_array")]
    url: Option<String>,
    #[serde(default)]
    recipe_yield: Option<Value>,
    #[serde(default, deserialize_with = "string_or_array")]
    prep_time: Option<String>,
    #[serde(default, deserialize_with = "string_or_array")]
    cook_time: Option<String>,
    #[serde(default, deserialize_with = "string_or_array")]
    total_time: Option<String>,
    #[serde(default, deserialize_with = "string_vec_or_array")]
    recipe_ingredient: Vec<String>,
    #[serde(default)]
    recipe_instructions: Option<Value>,
    #[serde(default)]
    keywords: Option<Value>,
    #[serde(default, deserialize_with = "string_or_array")]
    recipe_category: Option<String>,
    #[serde(default, deserialize_with = "string_or_array")]
    recipe_cuisine: Option<String>,
    #[serde(default)]
    aggregate_rating: Option<Value>,
}

fn string_or_array<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(Value::String(s)) => Some(s),
        Some(Value::Array(arr)) => arr.into_iter().find_map(|v| v.as_str().map(String::from)),
        _ => None,
    })
}

fn string_vec_or_array<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(Value::Array(arr)) => arr
            .into_iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s),
                Value::Array(inner) => inner.into_iter().find_map(|iv| iv.as_str().map(String::from)),
                _ => None,
            })
            .collect(),
        Some(Value::String(s)) => vec![s],
        _ => vec![],
    })
}

#[derive(Serialize)]
struct ImportResponse {
    imported_count: usize,
    failed: Vec<String>,
}

pub async fn import_recipesage(
    State(state): State<AppState>,
    body: String,
) -> impl IntoResponse {
    // Parse the JSON manually to avoid exposing private types in the signature
    let recipes: Vec<JsonLdRecipe> = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ImportResponse {
                    imported_count: 0,
                    failed: vec![format!("Invalid JSON: {}", e)],
                }),
            );
        }
    };

    let mut imported_count = 0;
    let mut failed = Vec::new();

    for recipe in recipes {
        match import_single_recipe(&state.pool, recipe).await {
            Ok(()) => imported_count += 1,
            Err(e) => failed.push(e),
        }
    }

    (
        StatusCode::OK,
        Json(ImportResponse {
            imported_count,
            failed,
        }),
    )
}

async fn import_single_recipe(
    pool: &SqlitePool,
    recipe: JsonLdRecipe,
) -> Result<(), String> {
    let title = recipe.name.clone().unwrap_or_else(|| "Untitled Recipe".to_string());

    // Convert ingredients to structured format
    let ingredients: Vec<Ingredient> = recipe
        .recipe_ingredient
        .iter()
        .map(|ing_str| Ingredient {
            quantity: None,
            unit: None,
            name: ing_str.clone(),
            prep: None,
        })
        .collect();

    let ingredients_json = serde_json::to_string(&ingredients)
        .map_err(|e| format!("{title}: Failed to serialize ingredients: {e}"))?;

    // Parse instructions from various formats
    let instructions = parse_instructions(recipe.recipe_instructions);
    let instructions_json = serde_json::to_string(&instructions)
        .map_err(|e| format!("{title}: Failed to serialize instructions: {e}"))?;

    // Build notes from various fields
    let mut notes_parts = Vec::new();
    
    if let Some(desc) = recipe.description {
        if !desc.is_empty() {
            notes_parts.push(desc);
        }
    }
    
    if let Some(prep) = recipe.prep_time {
        notes_parts.push(format!("Prep Time: {prep}"));
    }
    
    if let Some(cook) = recipe.cook_time {
        notes_parts.push(format!("Cook Time: {cook}"));
    }
    
    if let Some(total) = recipe.total_time {
        notes_parts.push(format!("Total Time: {total}"));
    }
    
    if let Some(keywords) = recipe.keywords {
        let tags = extract_keywords(keywords);
        if !tags.is_empty() {
            notes_parts.push(format!("Tags: {tags}"));
        }
    }
    
    if let Some(cat) = recipe.recipe_category {
        notes_parts.push(format!("Category: {cat}"));
    }
    
    if let Some(cuisine) = recipe.recipe_cuisine {
        notes_parts.push(format!("Cuisine: {cuisine}"));
    }
    
    if let Some(rating_val) = recipe.aggregate_rating {
        if let Some(rating) = extract_rating(rating_val) {
            notes_parts.push(format!("Rating: {rating}/5"));
        }
    }
    
    let notes = notes_parts.join("\n\n");
    let source = recipe.url.unwrap_or_default();
    let yield_str = recipe.recipe_yield
        .and_then(|y| match y {
            Value::String(s) => Some(s),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .unwrap_or_default();

    sqlx::query(
        r#"
        INSERT INTO recipes (title, source, "yield", notes, ingredients, instructions)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&title)
    .bind(&source)
    .bind(&yield_str)
    .bind(&notes)
    .bind(&ingredients_json)
    .bind(&instructions_json)
    .execute(pool)
    .await
    .map_err(|e| format!("{title}: Database error: {e}"))?;

    Ok(())
}

fn parse_instructions(instructions: Option<Value>) -> Vec<String> {
    match instructions {
        Some(Value::String(s)) => vec![s],
        Some(Value::Array(arr)) => arr
            .into_iter()
            .filter_map(|item| match item {
                Value::String(s) => Some(s),
                Value::Object(obj) => obj.get("text").and_then(|v| v.as_str()).map(std::string::ToString::to_string),
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
}

fn extract_keywords(keywords: Value) -> String {
    match keywords {
        Value::String(s) => s,
        Value::Array(arr) => arr
            .into_iter()
            .filter_map(|v| v.as_str().map(std::string::ToString::to_string))
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}

fn extract_rating(rating: Value) -> Option<i32> {
    if let Value::Object(obj) = rating {
        obj.get("ratingValue")
            .and_then(serde_json::Value::as_i64)
            .and_then(|n| i32::try_from(n).ok())
    } else {
        None
    }
}
