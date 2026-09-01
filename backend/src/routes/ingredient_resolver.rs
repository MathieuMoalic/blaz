use axum::http::StatusCode;
use sqlx::SqlitePool;
use std::collections::HashMap;
use tracing::warn;

use crate::llm::LlmClient;
use crate::models::Ingredient;
use crate::units::normalize_name;

#[derive(Clone, Debug)]
pub struct ResolvedIngredient {
    pub canonical_name: String,
    pub category: String,
}

/// Resolves ingredient canonical names and shopping categories for better shopping list merging.
/// Uses the `ingredient_aliases` table and LLM for unresolved names.
#[allow(dead_code)]
pub async fn resolve_ingredient_canonical_names(
    pool: &SqlitePool,
    llm_client: &LlmClient,
    http: &reqwest::Client,
    system_prompt: &str,
    ingredients: &mut [Ingredient],
) -> Result<(), StatusCode> {
    // Collect unique ingredient names (excluding sections)
    let mut names_to_resolve: HashMap<String, Vec<usize>> = HashMap::new();

    for (idx, ingredient) in ingredients.iter().enumerate() {
        if ingredient.section.is_none() && !ingredient.name.is_empty() {
            let norm_name = normalize_name(&ingredient.name);
            if !norm_name.is_empty() {
                names_to_resolve
                    .entry(norm_name)
                    .or_default()
                    .push(idx);
            }
        }
    }

    if names_to_resolve.is_empty() {
        return Ok(());
    }

    // Validate configured shopping categories
    let valid_categories: Vec<String> =
        sqlx::query_scalar("SELECT name FROM shopping_categories ORDER BY sort_order")
            .fetch_all(pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Resolve all names (including categories)
    let resolved = resolve_names_batch(
        pool,
        llm_client,
        http,
        system_prompt,
        names_to_resolve.keys(),
        &valid_categories,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Assign canonical names to ingredients
    for ingredient in ingredients.iter_mut() {
        if ingredient.section.is_none() && !ingredient.name.is_empty() {
            let norm_name = normalize_name(&ingredient.name);
            if let Some(resolved_ing) = resolved.get(&norm_name) {
                ingredient.canonical_name = Some(resolved_ing.canonical_name.clone());
            }
        }
    }

    Ok(())
}

/// Public function to resolve ingredient canonical names and categories for shopping merge.
/// Returns a map of `normalized_name` -> `ResolvedIngredient`.
#[allow(dead_code)]
pub async fn resolve_ingredient_shopping_categories(
    pool: &SqlitePool,
    llm_client: &LlmClient,
    http: &reqwest::Client,
    system_prompt: &str,
    ingredient_names: &[String],
) -> Result<HashMap<String, ResolvedIngredient>, StatusCode> {
    if ingredient_names.is_empty() {
        return Ok(HashMap::new());
    }

    // Validate configured shopping categories
    let valid_categories: Vec<String> =
        sqlx::query_scalar("SELECT name FROM shopping_categories ORDER BY sort_order")
            .fetch_all(pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Resolve all names (including categories)
    resolve_names_batch(
        pool,
        llm_client,
        http,
        system_prompt,
        ingredient_names.iter(),
        &valid_categories,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Batch resolve ingredient names and categories using alias table and LLM if needed.
/// Returns a map of `normalized_name` -> `ResolvedIngredient`.
#[allow(dead_code)]
async fn resolve_names_batch(
    pool: &SqlitePool,
    llm_client: &LlmClient,
    http: &reqwest::Client,
    system_prompt: &str,
    names: impl Iterator<Item = &String>,
    valid_categories: &[String],
) -> Result<HashMap<String, ResolvedIngredient>, Box<dyn std::error::Error>> {
    let mut result: HashMap<String, ResolvedIngredient> = HashMap::new();
    let mut unresolved: Vec<String> = Vec::new();

    // Phase 1: Check existing aliases (confirmed=1 always win)
    for name in names {
        if name.is_empty() {
            continue;
        }

        let record: Option<(String, i32, String, i32)> = sqlx::query_as(
            "SELECT canonical_name, confirmed, category, confirmed_category FROM ingredient_aliases WHERE raw_name = ?",
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;

        match record {
            Some((canonical_name, _confirmed, category, _confirmed_category)) => {
                // Use the cached result
                result.insert(
                    name.clone(),
                    ResolvedIngredient {
                        canonical_name,
                        category,
                    },
                );
            }
            None => {
                unresolved.push(name.clone());
            }
        }
    }

    // Phase 2: Resolve unresolved names using LLM
    if !unresolved.is_empty() {
        let llm_result = resolve_names_via_llm(
            llm_client,
            http,
            system_prompt,
            &unresolved,
            valid_categories,
        )
        .await?;

        // Save to alias table (confirmed=0, confirmed_category=0) and add to result
        for (raw_name, resolved_ing) in &llm_result {
            // Insert or ignore if already exists
            sqlx::query(
                "INSERT OR IGNORE INTO ingredient_aliases (raw_name, canonical_name, confirmed, category, confirmed_category) VALUES (?, ?, 0, ?, 0)",
            )
            .bind(raw_name)
            .bind(&resolved_ing.canonical_name)
            .bind(&resolved_ing.category)
            .execute(pool)
            .await?;

            result.insert(raw_name.clone(), resolved_ing.clone());
        }
    }

    Ok(result)
}

/// Call LLM to normalize ingredient names and assign shopping categories.
/// Returns a map of `input_name` -> `ResolvedIngredient` (`canonical_name` + category).
#[allow(dead_code)]
async fn resolve_names_via_llm(
    llm_client: &LlmClient,
    http: &reqwest::Client,
    system_prompt: &str,
    names: &[String],
    valid_categories: &[String],
) -> Result<HashMap<String, ResolvedIngredient>, Box<dyn std::error::Error>> {
    let valid_categories_str = valid_categories.join(", ");
    let ingredient_list = names
        .iter()
        .enumerate()
        .map(|(i, n)| format!("{i}: \"{n}\""))
        .collect::<Vec<_>>()
        .join("\n");
    let user_prompt = format!(
        "For each ingredient, return BOTH the canonical name (singular, no size/prep descriptors) AND the shopping category.\n\nValid categories: {valid_categories_str}\n\nIngredients:\n{ingredient_list}"
    );

    let response = llm_client
        .chat_json(
            http,
            system_prompt,
            &user_prompt,
            0.3,
            std::time::Duration::from_secs(30),
            None,
        )
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { format!("LLM call failed: {e}").into() })?;

    // Parse response: expect {"0": {"canonical": "apple", "category": "Fruits"}, ...}
    let mut result: HashMap<String, ResolvedIngredient> = HashMap::new();

    if let Some(obj) = response.as_object() {
        for (idx_str, resolved_val) in obj {
            if let Ok(idx) = idx_str.parse::<usize>()
                && idx < names.len()
                && let Some(obj_val) = resolved_val.as_object()
                && let (Some(canonical_str), Some(category_str)) = (
                    obj_val.get("canonical").and_then(|v| v.as_str()),
                    obj_val.get("category").and_then(|v| v.as_str()),
                )
            {
                let canonical = normalize_name(canonical_str);
                if !canonical.is_empty() {
                    // Validate category
                    let validated_category = if valid_categories.contains(&category_str.to_string())
                    {
                        category_str.to_string()
                    } else {
                        warn!(
                            "LLM returned invalid category '{category_str}' for ingredient '{}', falling back to 'Other'",
                            names[idx]
                        );
                        "Other".to_string()
                    };

                    result.insert(
                        names[idx].clone(),
                        ResolvedIngredient {
                            canonical_name: canonical,
                            category: validated_category,
                        },
                    );
                }
            }
        }
    }

    Ok(result)
}
