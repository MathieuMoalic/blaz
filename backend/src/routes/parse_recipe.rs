use crate::error::AppResult;
use crate::html::{clean_title, extract_title, fallback_title_from_url, html_to_plain_text};
use crate::llm::LlmClient;
use crate::models::Ingredient;
use crate::{
    models::{AppState, NewRecipe, Recipe},
    routes::{parse_recipe_image::extract_main_image_url, recipes},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::time::Duration;

/* =========================
 * Request DTO
 * ========================= */

#[derive(Deserialize)]
pub struct ImportFromUrlReq {
    pub url: String,
    /// Optional model override (e.g., "deepseek/deepseek-chat-v3.1")
    #[serde(default)]
    pub model: Option<String>,
    /// When true, parse the URL but do NOT persist to the database.
    /// Returns a Recipe with id=0. Use this for re-import (updating an existing recipe).
    #[serde(default)]
    pub dry_run: bool,
}

/// # Errors
///
/// Err if we can't fetch from the url
pub async fn import_from_url(
    State(state): State<AppState>,
    Json(req): Json<ImportFromUrlReq>,
) -> AppResult<Json<Recipe>> {
    const MAX_CHARS: usize = 12_000;

    let (title_guess_raw, text, html) = fetch_page_text(&req.url)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("fetch failed: {e}")))?;

    let title_guess = clean_title(&title_guess_raw);

    if text.trim().is_empty() {
        return Err((StatusCode::BAD_GATEWAY, "page has no readable text".into()).into());
    }

    let token = state.config.llm_api_key.clone().unwrap_or_default();
    if token.is_empty() {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "LLM API key is not configured (use --llm-api-key or BLAZ_LLM_API_KEY)".into(),
        )
            .into());
    }

    let model = req.model.as_deref().unwrap_or(&state.config.llm_model);
    let base = state.config.llm_api_url.as_str();

    let excerpt = if text.len() > MAX_CHARS {
        &text[..MAX_CHARS]
    } else {
        &text
    };

    let http = reqwest::Client::new();
    let llm = LlmClient::new(base.to_string(), token.clone(), model.to_string());

    // STAGE 1: Extract raw text
    tracing::info!("Stage 1: Extracting raw text");
    let (title, ingredient_strings, instruction_strings) = 
        stage1_extract(&llm, &http, &state, excerpt, &req.url, &title_guess)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Stage 1 (extract) failed: {e}")))?;

    // STAGE 2: Structure ingredients
    tracing::info!("Stage 2: Structuring {} ingredients", ingredient_strings.len());
    let mut structured_ingredients =
        stage2_structure_ingredients(&llm, &http, &state, &ingredient_strings)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Stage 2 (structure) failed: {e}")))?;

    // STAGE 3: Convert to metric
    tracing::info!("Stage 3: Converting to metric");
    structured_ingredients =
        stage3_convert_to_metric(&llm, &http, &state, &structured_ingredients)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Stage 3 (convert) failed: {e}")))?;

    let final_title = if title.trim().is_empty() {
        fallback_title_from_url(&req.url).unwrap_or_else(|| "Imported recipe".to_string())
    } else {
        title
    };

    let payload = NewRecipe {
        title: final_title,
        source: req.url.clone(),
        r#yield: String::new(),
        notes: String::new(),
        ingredients: structured_ingredients,
        instructions: instruction_strings,
    };

    if req.dry_run {
        // Caller wants the parsed data but will manage persistence themselves.
        // Return a transient Recipe (id=0) without writing to the database.
        let recipe = Recipe {
            id: 0,
            title: payload.title,
            source: payload.source,
            r#yield: payload.r#yield,
            notes: payload.notes,
            created_at: String::new(),
            updated_at: String::new(),
            ingredients: payload.ingredients,
            instructions: payload.instructions,
            image_path_small: None,
            image_path_full: None,
            macros: None,
            share_token: None,
            prep_reminders: None,
        };
        return Ok(Json(recipe));
    }

    let created = recipes::create(State(state.clone()), Json(payload)).await?;
    let recipe_id = created.0.id;

    if let Err(e) = try_fetch_and_attach_image(&state, recipe_id, &req.url, &html).await {
        tracing::warn!("image import failed for id {}: {}", recipe_id, e);
    }

    let fresh = recipes::get(State(state), Path(recipe_id)).await?;
    Ok(fresh)
}

/* =========================
 * HTML fetch + plain text
 * ========================= */

async fn fetch_page_text(url: &str) -> Result<(String, String, String), String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(45))
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {} fetching {}", resp.status(), url));
    }

    let html = resp.text().await.unwrap_or_default();
    let title = extract_title(&html).unwrap_or_default();
    let text = html_to_plain_text(&html);

    Ok((title, text, html))
}

/* =========================
 * Image: reuse parse_recipe_image.rs
 * ========================= */

async fn try_fetch_and_attach_image(
    state: &AppState,
    recipe_id: i64,
    page_url: &str,
    html: &str,
) -> anyhow::Result<()> {
    if let Some(img_url) = extract_main_image_url(html, page_url) {
        let client = reqwest::Client::new();

        // Download + generate stable full + small images under:
        //   media/recipes/<id>/full.webp
        //   media/recipes/<id>/small.webp
        let (rel_full, rel_small) =
            recipes::fetch_and_store_recipe_image(&client, &img_url, state, recipe_id).await?;

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

        return Ok(());
    }

    anyhow::bail!("no image candidate found by extract_main_image_url")
}

/* =========================
 * Stage 1: Extract raw text
 * ========================= */

async fn stage1_extract(
    llm: &LlmClient,
    http: &reqwest::Client,
    state: &AppState,
    content: &str,
    url: &str,
    title_guess: &str,
) -> anyhow::Result<(String, Vec<String>, Vec<String>)> {
    let user = format!(
        "URL: {url}\nTITLE: {title_guess}\n\nCONTENT:\n{content}"
    );

    let json = call_llm_with_retry(
        llm,
        http,
        &state.config.system_prompt_extract,
        &user,
        0.1,
        Duration::from_secs(120),
        Some(16_000),
    )
    .await?;

    let title = json
        .get("title")
        .and_then(|v| v.as_str())
        .map(clean_title)
        .unwrap_or_default();

    let ingredients = json
        .get("ingredients")
        .and_then(|v| v.as_array())
        .map_or_else(Vec::new, |arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect::<Vec<String>>()
        });

    let instructions = json
        .get("instructions")
        .and_then(|v| v.as_array())
        .map_or_else(Vec::new, |arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect::<Vec<String>>()
        });

    validate_stage1(&ingredients, &instructions)?;
    
    Ok((title, ingredients, instructions))
}

/* =========================
 * Stage 2: Structure ingredients
 * ========================= */

async fn stage2_structure_ingredients(
    llm: &LlmClient,
    http: &reqwest::Client,
    state: &AppState,
    ingredient_strings: &[String],
) -> anyhow::Result<Vec<Ingredient>> {
    let input_json = serde_json::to_string(ingredient_strings)?;
    
    let json = call_llm_with_retry(
        llm,
        http,
        &state.config.system_prompt_structure,
        &input_json,
        0.1,
        Duration::from_secs(120),
        Some(16_000),
    )
    .await?;

    let ingredients = json.as_array().map_or_else(
        || {
            json.get("ingredients")
                .and_then(|v| v.as_array())
                .map_or_else(Vec::new, |_arr| {
                    normalize_ingredients(
                        json.get("ingredients").cloned().unwrap_or(JsonValue::Null),
                    )
                })
        },
        |_arr| normalize_ingredients(json.clone()),
    );

    validate_stage2(&ingredients);
    
    Ok(ingredients)
}

/* =========================
 * Stage 3: Convert to metric
 * ========================= */

async fn stage3_convert_to_metric(
    llm: &LlmClient,
    http: &reqwest::Client,
    state: &AppState,
    ingredients: &[Ingredient],
) -> anyhow::Result<Vec<Ingredient>> {
    // Serialize current ingredients as JSON
    let ingredients_json: Vec<JsonValue> = ingredients
        .iter()
        .map(|ing| {
            ing.section.as_ref().map_or_else(
                || {
                    serde_json::json!({
                        "quantity": ing.quantity,
                        "unit": ing.unit,
                        "name": ing.name,
                        "prep": ing.prep,
                    })
                },
                |section| serde_json::json!({"section": section}),
            )
        })
        .collect();

    let input_json = serde_json::to_string(&ingredients_json)?;

    let json = call_llm_with_retry(
        llm,
        http,
        &state.config.system_prompt_convert,
        &input_json,
        0.1,
        Duration::from_secs(120),
        Some(16_000),
    )
    .await?;

    let converted = json.as_array().map_or_else(
        || {
            json.get("ingredients")
                .and_then(|v| v.as_array())
                .map_or_else(Vec::new, |_arr| {
                    normalize_ingredients(
                        json.get("ingredients").cloned().unwrap_or(JsonValue::Null),
                    )
                })
        },
        |_arr| normalize_ingredients(json.clone()),
    );

    validate_stage3(&converted)?;
    
    Ok(converted)
}

/* =========================
 * Retry wrapper
 * ========================= */

async fn call_llm_with_retry(
    llm: &LlmClient,
    http: &reqwest::Client,
    system: &str,
    user: &str,
    temperature: f32,
    timeout: Duration,
    max_tokens: Option<u32>,
) -> anyhow::Result<JsonValue> {
    // Try once
    match llm.chat_json(http, system, user, temperature, timeout, max_tokens).await {
        Ok(json) => Ok(json),
        Err(e) => {
            tracing::warn!("LLM call failed, retrying once: {}", e);
            // Retry once
            llm.chat_json(http, system, user, temperature, timeout, max_tokens).await
        }
    }
}

/* =========================
 * Validation functions
 * ========================= */

fn validate_stage1(ingredients: &[String], instructions: &[String]) -> anyhow::Result<()> {
    if ingredients.is_empty() {
        anyhow::bail!("Stage 1 returned no ingredients");
    }
    if instructions.is_empty() {
        anyhow::bail!("Stage 1 returned no instructions");
    }
    // Check for reasonable counts
    if ingredients.len() > 200 {
        anyhow::bail!("Stage 1 returned too many ingredients ({})", ingredients.len());
    }
    if instructions.len() > 200 {
        anyhow::bail!("Stage 1 returned too many instructions ({})", instructions.len());
    }
    Ok(())
}

const BANNED_UNITS: &[&str] = &["cup", "cups", "oz", "ounce", "ounces", "fl oz", "fluid ounce", "pound", "lb", "lbs", "pint", "quart", "gallon"];
const PREP_WORDS: &[&str] = &["sliced", "diced", "minced", "chopped", "grated", "shredded", "softened", "melted"];

fn validate_stage2(ingredients: &[Ingredient]) {
    // Check for banned units (warning only, not fatal)
    
    for ing in ingredients {
        if let Some(unit) = &ing.unit {
            let unit_lower = unit.to_lowercase();
            if BANNED_UNITS.contains(&unit_lower.as_str()) {
                tracing::warn!("Stage 2 validation: ingredient '{}' has banned unit '{}'", ing.name, unit);
            }
        }
        
        // Check name doesn't contain prep words (warning only)
        if !ing.name.is_empty() {
            let name_lower = ing.name.to_lowercase();
            for prep_word in PREP_WORDS {
                if name_lower.contains(prep_word) {
                    tracing::warn!("Stage 2 validation: ingredient name '{}' contains prep word '{}'", ing.name, prep_word);
                }
            }
        }
    }
}

fn validate_stage3(ingredients: &[Ingredient]) -> anyhow::Result<()> {
    const ALLOWED_UNITS: &[&str] = &["g", "kg", "ml", "l", "tsp", "tbsp", "tablespoon", "teaspoon"];
    const BANNED_UNITS: &[&str] = &["cup", "cups", "oz", "ounce", "ounces", "fl oz", "fluid ounce", "pound", "lb", "lbs", "pint", "quart", "gallon"];
    
    for ing in ingredients {
        if let Some(unit) = &ing.unit {
            let unit_lower = unit.to_lowercase();
            
            if BANNED_UNITS.contains(&unit_lower.as_str()) {
                anyhow::bail!(
                    "Stage 3 validation failed: ingredient '{}' still has banned unit '{}'",
                    ing.name,
                    unit
                );
            }
            
            if !ALLOWED_UNITS.contains(&unit_lower.as_str()) {
                tracing::warn!(
                    "Stage 3 validation: ingredient '{}' has non-standard unit '{}'",
                    ing.name,
                    unit
                );
            }
        }
    }
    
    Ok(())
}

/* =========================
 * Tolerant normalization (+ optional title)
 * ========================= */

#[derive(Default, Clone)]
pub struct ExtractRaw {
    pub title: Option<String>,
    pub ingredients: JsonValue,
    pub instructions: JsonValue,
}

impl ExtractRaw {
    pub fn from_json(v: &JsonValue) -> Self {
        let title = v
            .get("title")
            .and_then(|x| x.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty());

        Self {
            title,
            ingredients: v.get("ingredients").cloned().unwrap_or(JsonValue::Null),
            instructions: v.get("instructions").cloned().unwrap_or(JsonValue::Null),
        }
    }

    pub fn normalize(self) -> ExtractOut {
        ExtractOut {
            ingredients: normalize_ingredients(self.ingredients),
            instructions: normalize_instructions(self.instructions),
        }
    }
}

pub struct ExtractOut {
    pub ingredients: Vec<Ingredient>,
    pub instructions: Vec<String>,
}

pub fn normalize_instructions(v: JsonValue) -> Vec<String> {
    match v {
        JsonValue::Array(items) => items
            .into_iter()
            .filter_map(|x| match x {
                JsonValue::String(s) => {
                    let t = s.trim().to_string();
                    (!t.is_empty()).then_some(t)
                }
                // {"section": "Sauce"} → "## Sauce"
                JsonValue::Object(m) => {
                    m.get("section")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|s| format!("## {s}"))
                }
                JsonValue::Number(n) => Some(n.to_string()),
                JsonValue::Bool(b) => Some(b.to_string()),
                _ => None,
            })
            .collect(),
        JsonValue::String(s) => s
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(std::string::ToString::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

pub fn normalize_ingredients(v: JsonValue) -> Vec<Ingredient> {
    match v {
        JsonValue::Array(items) => items
            .into_iter()
            .filter_map(|x| match x {
                JsonValue::Object(mut m) => {
                    // Section header: {"section": "Sauce"}
                    if let Some(s) = m.get("section").and_then(|v| v.as_str()) {
                        let label = s.trim().to_string();
                        if !label.is_empty() {
                            return Some(Ingredient {
                                section: Some(label),
                                quantity: None,
                                unit: None,
                                name: String::new(),
                                prep: None,
                                raw: false,
                            });
                        }
                    }

                    let name = m
                        .remove("name")
                        .and_then(|v| v.as_str().map(|s| s.trim().to_string()))
                        .unwrap_or_default();

                    if name.is_empty() {
                        return None;
                    }

                    let quantity = m
                        .remove("quantity")
                        .or_else(|| m.remove("qty"))
                        .or_else(|| m.remove("amount"))
                        .and_then(|v| match v {
                            JsonValue::Number(n) => n.as_f64(),
                            JsonValue::String(s) => s.trim().replace(',', ".").parse::<f64>().ok(),
                            _ => None,
                        });

                    let unit = m
                        .remove("unit")
                        .and_then(|v| v.as_str().map(|s| s.trim().to_string()))
                        .filter(|s| !s.is_empty());

                    let prep = m
                        .remove("prep")
                        .and_then(|v| v.as_str().map(|s| s.trim().to_string()))
                        .filter(|s| !s.is_empty());

                    Some(Ingredient {
                        section: None,
                        quantity,
                        unit,
                        name,
                        prep,
                        raw: false,
                    })
                }
                _ => None, // NO STRINGS ACCEPTED
            })
            .collect(),
        _ => Vec::new(),
    }
}
