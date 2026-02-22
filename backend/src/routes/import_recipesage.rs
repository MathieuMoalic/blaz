use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;

use crate::models::{AppState, Ingredient};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct JsonLdRecipe {
    #[serde(default, deserialize_with = "string_or_array")]
    name: Option<String>,
    #[serde(default, deserialize_with = "string_or_array")]
    url: Option<String>,
    #[serde(default, deserialize_with = "string_or_array")]
    is_based_on: Option<String>,
    #[serde(default)]
    recipe_yield: Option<Value>,
    #[serde(default, deserialize_with = "string_vec_or_array")]
    recipe_ingredient: Vec<String>,
    #[serde(default)]
    recipe_instructions: Option<Value>,
    #[serde(default, deserialize_with = "string_or_array")]
    notes: Option<String>,
    #[serde(default, deserialize_with = "string_or_array")]
    image: Option<String>,
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
                    failed: vec![format!("Invalid JSON: {e}")],
                }),
            );
        }
    };

    tracing::info!("Starting RecipeSage import of {} recipes", recipes.len());

    let mut imported_count = 0;
    let mut failed = Vec::new();

    for recipe in recipes {
        match import_single_recipe(&state, recipe).await {
            Ok(()) => imported_count += 1,
            Err(e) => {
                tracing::error!("Import failed: {}", e);
                failed.push(e);
            }
        }
    }

    tracing::info!(
        "RecipeSage import complete: {} succeeded, {} failed",
        imported_count,
        failed.len()
    );

    (
        StatusCode::OK,
        Json(ImportResponse {
            imported_count,
            failed,
        }),
    )
}

async fn import_single_recipe(
    state: &AppState,
    recipe: JsonLdRecipe,
) -> Result<(), String> {
    let title = recipe.name.clone().unwrap_or_else(|| "Untitled Recipe".to_string());
    
    tracing::info!("Importing recipe: {}", title);

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

    // Use only the notes field from RecipeSage
    let notes = recipe.notes.unwrap_or_default();
    
    // Prefer isBasedOn over url for source
    let source = recipe.is_based_on
        .or(recipe.url)
        .unwrap_or_default();
    
    if !source.is_empty() {
        tracing::info!("  Source URL: {}", source);
    }
    
    let yield_str = recipe.recipe_yield
        .and_then(|y| match y {
            Value::String(s) => Some(s),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .unwrap_or_default();

    let result = sqlx::query(
        r#"
        INSERT INTO recipes (title, source, "yield", notes, ingredients, instructions)
        VALUES (?, ?, ?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(&title)
    .bind(&source)
    .bind(&yield_str)
    .bind(&notes)
    .bind(&ingredients_json)
    .bind(&instructions_json)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| format!("{title}: Database error: {e}"))?;

    let recipe_id: i64 = result.get("id");
    tracing::info!("  Created recipe with ID: {}", recipe_id);

    // Import image - if there's a URL source, fetch from web; otherwise use local image
    if !source.is_empty() && (source.starts_with("http://") || source.starts_with("https://")) {
        // Fetch image from the source URL
        tracing::info!("  Fetching image from URL: {}", source);
        if let Err(e) = import_image_from_url(state, recipe_id, &source).await {
            tracing::warn!(recipe_id, source, error = %e, "Failed to import image from URL");
        } else {
            tracing::info!("  ✓ Image imported from URL");
        }
    } else if let Some(image_url) = recipe.image {
        // Use local image from recipeImage directory
        tracing::info!("  Using local image: {}", image_url);
        if let Err(e) = import_recipe_image(state, recipe_id, &image_url).await {
            tracing::warn!(recipe_id, image_url, error = %e, "Failed to import local image");
        } else {
            tracing::info!("  ✓ Local image imported");
        }
    } else {
        tracing::info!("  No image available");
    }

    tracing::info!("✓ Successfully imported: {}", title);
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

async fn import_image_from_url(
    state: &AppState,
    recipe_id: i64,
    source_url: &str,
) -> anyhow::Result<()> {
    // Fetch the page HTML to extract the image
    let client = reqwest::Client::new();
    let resp = client
        .get(source_url)
        .timeout(std::time::Duration::from_secs(45))
        .send()
        .await?;
    
    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("HTTP {} fetching {}", resp.status(), source_url));
    }
    
    let html = resp.text().await?;
    
    // Use the same image extraction logic as URL import
    if let Some(img_url) = crate::routes::parse_recipe_image::extract_main_image_url(&html, source_url) {
        let (rel_full, rel_small) =
            crate::routes::recipes::fetch_and_store_recipe_image(&client, &img_url, state, recipe_id).await?;
        
        sqlx::query(
            r"
            UPDATE recipes
            SET image_path_small = ?,
                image_path_full  = ?
            WHERE id = ?
            ",
        )
        .bind(&rel_small)
        .bind(&rel_full)
        .bind(recipe_id)
        .execute(&state.pool)
        .await?;
    }
    
    Ok(())
}

async fn import_recipe_image(
    state: &AppState,
    recipe_id: i64,
    image_url: &str,
) -> anyhow::Result<()> {
    let bytes = if let Some(data_uri) = image_url.strip_prefix("data:") {
        // Handle base64-encoded data URI
        // Format: "data:image/png;base64,..."
        let parts: Vec<&str> = data_uri.split(',').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!("Invalid data URI format"));
        }
        
        // Decode base64
        base64::engine::general_purpose::STANDARD
            .decode(parts[1])
            .map_err(|e| anyhow::anyhow!("Failed to decode base64: {e}"))?
    } else {
        // Handle file path (for local RecipeSage files)
        let path = image_url
            .strip_prefix("/api/")
            .or_else(|| image_url.strip_prefix("api/"))
            .unwrap_or(image_url);
        
        let image_path = std::path::Path::new("recipeImage").join(path.strip_prefix("recipeImage/").unwrap_or(path));
        
        if !image_path.exists() {
            return Err(anyhow::anyhow!("Image file not found: {}", image_path.display()));
        }

        tokio::fs::read(&image_path).await?
    };
    
    // Process and store using the existing image processing logic
    store_recipe_image_bytes(state, recipe_id, bytes).await?;
    
    Ok(())
}

async fn store_recipe_image_bytes(
    state: &AppState,
    recipe_id: i64,
    bytes: Vec<u8>,
) -> anyhow::Result<()> {
    let (full_webp, thumb_webp) =
        tokio::task::spawn_blocking(move || -> std::io::Result<(Vec<u8>, Vec<u8>)> {
            let img = image::load_from_memory(&bytes)
                .map_err(|e| std::io::Error::other(format!("decode error: {e}")))?;
            crate::image_io::to_full_and_thumb_webp(&img)
        })
        .await??;

    let rel_dir = format!("recipes/{recipe_id}");
    let rel_full = format!("{rel_dir}/full.webp");
    let rel_small = format!("{rel_dir}/small.webp");

    let abs_dir = state.config.media_dir.join(&rel_dir);
    tokio::fs::create_dir_all(&abs_dir).await?;
    tokio::fs::write(abs_dir.join("full.webp"), &full_webp).await?;
    tokio::fs::write(abs_dir.join("small.webp"), &thumb_webp).await?;

    // Update the recipe with image paths
    sqlx::query(
        r"
        UPDATE recipes
        SET image_path_full = ?, image_path_small = ?
        WHERE id = ?
        ",
    )
    .bind(&rel_full)
    .bind(&rel_small)
    .bind(recipe_id)
    .execute(&state.pool)
    .await?;

    Ok(())
}
