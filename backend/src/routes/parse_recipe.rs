use crate::html::{clean_title, extract_title, fallback_title_from_url, html_to_plain_text};
use crate::llm::LlmClient;
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
}

/* =========================
 * Handler
 * ========================= */

/// # Errors
///
/// Err if we can't fetch from the url
pub async fn import_from_url(
    State(state): State<AppState>,
    Json(req): Json<ImportFromUrlReq>,
) -> Result<Json<Recipe>, (StatusCode, String)> {
    const MAX_CHARS: usize = 12_000;
    // 1) Fetch page HTML and convert to plain text (also return raw html)
    let (title_guess_raw, text, html) = fetch_page_text(&req.url)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("fetch failed: {e}")))?;

    // Clean the HTML title (remove branding & adjectives)
    let title_guess = clean_title(&title_guess_raw);

    if text.trim().is_empty() {
        return Err((StatusCode::BAD_GATEWAY, "page has no readable text".into()));
    }

    // 2) Read runtime LLM settings
    let settings = state.settings.read().await.clone();

    let token = settings.llm_api_key.clone().unwrap_or_default();
    if token.is_empty() {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "LLM API key is not configured (set it in /app-state)".into(),
        ));
    }

    let model = req.model.as_deref().unwrap_or(&settings.llm_model);
    let base = settings.llm_api_url.as_str();
    let system = settings.system_prompt_import.as_str();

    // 3) Compact user message
    let excerpt = if text.len() > MAX_CHARS {
        &text[..MAX_CHARS]
    } else {
        &text
    };
    let user = format!(
        "URL: {url}\nTITLE: {title}\n\nCONTENT:\n{content}",
        url = req.url,
        title = title_guess,
        content = excerpt
    );

    // 4) Call LLM -> JSON
    let http = reqwest::Client::new();
    let llm = LlmClient::new(base.to_string(), token.clone(), model.to_string());

    let llm_json = llm
        .chat_json(
            &http,
            system,
            &user,
            0.1,
            Duration::from_secs(120),
            Some(500),
        )
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("LLM extract failed: {e}")))?;

    // 5) Normalize (and capture possible LLM title)
    let raw = ExtractRaw::from_json(&llm_json);
    let title_from_llm = raw.title.clone();
    let norm = raw.normalize();

    let chosen_title = title_from_llm
        .as_deref()
        .map_or_else(|| title_guess.clone(), clean_title);

    let final_title = if chosen_title.trim().is_empty() {
        fallback_title_from_url(&req.url).unwrap_or_else(|| "Imported recipe".to_string())
    } else {
        chosen_title
    };

    let payload = NewRecipe {
        title: final_title,
        source: req.url.clone(),
        r#yield: String::new(),
        notes: String::new(),
        ingredients: norm.ingredients,
        instructions: norm.instructions,
    };

    // 6) Create recipe
    let created = recipes::create(State(state.clone()), Json(payload))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}")))?; // <- use Debug
    let recipe_id = created.0.id;

    // 7) Import hero image using your parse_recipe_image helper (best-effort)
    if let Err(e) = try_fetch_and_attach_image(&state, recipe_id, &req.url, &html).await {
        tracing::warn!("image import failed for id {}: {}", recipe_id, e);
    }

    // 8) Return the fresh row (with image paths if saved)
    let fresh = recipes::get(State(state), Path(recipe_id))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:?}")))?; // <- use Debug
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
 * Tolerant normalization (+ optional title)
 * ========================= */

#[derive(Default, Clone)]
struct ExtractRaw {
    title: Option<String>,
    ingredients: JsonValue,
    instructions: JsonValue,
}

impl ExtractRaw {
    fn from_json(v: &JsonValue) -> Self {
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

    fn normalize(self) -> ExtractOut {
        ExtractOut {
            ingredients: normalize_ingredients(self.ingredients),
            instructions: normalize_instructions(self.instructions),
        }
    }
}

struct ExtractOut {
    ingredients: Vec<String>,
    instructions: Vec<String>,
}

fn normalize_instructions(v: JsonValue) -> Vec<String> {
    match v {
        JsonValue::Array(items) => items
            .into_iter()
            .filter_map(|x| match x {
                JsonValue::String(s) => {
                    let t = s.trim().to_string();
                    (!t.is_empty()).then_some(t)
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

fn normalize_ingredients(v: JsonValue) -> Vec<String> {
    match v {
        JsonValue::Array(items) => items
            .into_iter()
            .filter_map(|x| match x {
                JsonValue::String(s) => {
                    let t = s.trim().to_string();
                    (!t.is_empty()).then_some(t)
                }
                JsonValue::Object(mut m) => {
                    let name = m
                        .remove("name")
                        .and_then(|v| v.as_str().map(|s| s.trim().to_string()))
                        .unwrap_or_default();
                    if name.is_empty() {
                        return None;
                    }
                    let q = m
                        .remove("quantity")
                        .or_else(|| m.remove("qty"))
                        .or_else(|| m.remove("amount"))
                        .and_then(|v| match v {
                            JsonValue::Number(n) => n.as_f64(),
                            JsonValue::String(s) => s.trim().parse::<f64>().ok(),
                            _ => None,
                        });
                    let unit = m
                        .remove("unit")
                        .and_then(|v| v.as_str().map(|s| s.trim().to_string()))
                        .filter(|s| !s.is_empty());

                    Some(to_line(q, unit, name))
                }
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

fn to_line(q: Option<f64>, unit: Option<String>, name: String) -> String {
    fn trim_zeros(mut s: String) -> String {
        if s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
        s
    }
    match (q, unit) {
        (Some(v), Some(u)) if !u.is_empty() => {
            let s = if u == "g" || u == "ml" {
                v.round().to_string()
            } else if u == "kg" || u == "L" {
                trim_zeros(format!("{v:.2}"))
            } else {
                trim_zeros(format!("{}", ((v * 100.0).round() / 100.0)))
            };
            format!("{s} {u} {name}")
        }
        (Some(v), None) => {
            let s = trim_zeros(format!("{}", ((v * 100.0).round() / 100.0)));
            format!("{s} {name}")
        }
        _ => name,
    }
}
