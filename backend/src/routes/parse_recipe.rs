use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use std::{path::PathBuf, time::Duration};

use crate::{
    models::{AppState, NewRecipe, Recipe},
    routes::{parse_recipe_image::extract_main_image_url, recipes},
};

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

pub async fn import_from_url(
    State(state): State<AppState>,
    Json(req): Json<ImportFromUrlReq>,
) -> Result<Json<Recipe>, (StatusCode, String)> {
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
    const MAX_CHARS: usize = 12_000;
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
    let client = reqwest::Client::new();
    let llm_json = call_llm_json(&client, base, &token, model, system, &user)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("LLM extract failed: {e}")))?;

    // 5) Normalize (and capture possible LLM title)
    let raw = ExtractRaw::from_json(&llm_json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("invalid LLM JSON: {e}")))?;
    let title_from_llm = raw.title.clone();
    let norm = raw.normalize();

    let chosen_title = title_from_llm
        .as_deref()
        .map(clean_title)
        .unwrap_or_else(|| title_guess.clone());

    let final_title = if !chosen_title.trim().is_empty() {
        chosen_title
    } else {
        fallback_title_from_url(&req.url).unwrap_or_else(|| "Imported recipe".to_string())
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

fn extract_title(html: &str) -> Option<String> {
    static TITLE_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap());
    let raw = TITLE_RE
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())?;
    Some(decode_entities_basic(&raw))
}

fn fallback_title_from_url(url: &str) -> Option<String> {
    if let Ok(u) = reqwest::Url::parse(url) {
        let host = u.host_str().unwrap_or_default().to_string();
        let p = u.path().trim_matches('/');
        if p.is_empty() {
            Some(host)
        } else {
            Some(format!("{host} — {p}"))
        }
    } else {
        None
    }
}

fn html_to_plain_text(html: &str) -> String {
    static SCRIPT_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap());
    static STYLE_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap());
    static TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?is)<[^>]+>").unwrap());
    static WS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[ \t\r\f]+").unwrap());
    static NL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n{3,}").unwrap());

    let mut s = SCRIPT_RE.replace_all(html, " ").into_owned();
    s = STYLE_RE.replace_all(&s, " ").into_owned();
    s = TAG_RE.replace_all(&s, "\n").into_owned();
    s = decode_entities_basic(&s);
    s = WS_RE.replace_all(&s, " ").into_owned();
    s = s.replace("\r\n", "\n").replace('\r', "\n");
    s = NL_RE.replace_all(&s, "\n\n").into_owned();
    s.trim().to_string()
}

fn decode_entities_basic(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#039;", "'")
        .replace("&#x27;", "'")
        .replace("&#8211;", "–")
        .replace("&#8212;", "—")
        .replace("&#8226;", "•")
        .replace("&nbsp;", " ")
}

/* =========================
 * Title normalization
 * ========================= */

fn clean_title(input: &str) -> String {
    let mut s = decode_entities_basic(input).trim().to_string();

    // Cut at common separators (keep the left part)
    let seps = ['•', '|', '—', '–', ':'];
    if let Some(idx) = s.find(|c| seps.contains(&c)) {
        s = s[..idx].trim().to_string();
    }

    // Strip adjectives / diet tags
    static ADJ_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(
            r"(?i)^(best|easy|quick|simple|ultimate|perfect|authentic|classic|vegan|keto|paleo|gluten[- ]free)\s+",
        )
        .unwrap()
    });
    loop {
        let new = ADJ_RE.replace(&s, "").trim().to_string();
        if new == s {
            break;
        }
        s = new;
    }

    static RECIPE_TAIL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\s+recipes?$").unwrap());
    s = RECIPE_TAIL_RE.replace(&s, "").trim().to_string();

    // Normalize whitespace & capitalize first letter
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(first) = s.get(..1) {
        s = format!("{}{}", first.to_uppercase(), s.get(1..).unwrap_or(""));
    }
    s
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
        let resp = client
            .get(&img_url)
            .timeout(Duration::from_secs(40))
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("image HTTP {}", resp.status());
        }

        // Clone content-type into an owned String BEFORE consuming the response
        let ct: String = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_default();

        if !ct.starts_with("image/") {
            anyhow::bail!("not an image content-type: {}", ct);
        }

        // Now it's safe to consume the response
        let bytes = resp.bytes().await?;
        if bytes.is_empty() {
            anyhow::bail!("empty image body");
        }

        let ext = guess_ext(&ct).unwrap_or_else(|| guess_ext_from_url(&img_url).unwrap_or("jpg"));

        let rel_dir = format!("recipes/{}", recipe_id);
        let full_name = format!("full.{}", ext);
        let rel_full = format!("{}/{}", rel_dir, full_name);

        let media_root = PathBuf::from(&state.config.media_dir);
        let dir = media_root.join(&rel_dir);
        tokio::fs::create_dir_all(&dir).await?;
        let full_path = media_root.join(&rel_full);
        tokio::fs::write(&full_path, &bytes).await?;

        // For now, point "small" to the same file
        let rel_small = rel_full.clone();

        // Update DB paths
        sqlx::query(
            r#"
            UPDATE recipes
               SET image_path_small = ?,
                   image_path_full  = ?
             WHERE id = ?
            "#,
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

fn guess_ext(ct: &str) -> Option<&'static str> {
    match ct.split(';').next().unwrap_or("").trim() {
        "image/jpeg" => Some("jpg"),
        "image/jpg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        "image/avif" => Some("avif"),
        "image/gif" => Some("gif"),
        _ => None,
    }
}

fn guess_ext_from_url(u: &str) -> Option<&'static str> {
    let lower = u.to_lowercase();
    for (needle, ext) in [
        (".jpg", "jpg"),
        (".jpeg", "jpg"),
        (".png", "png"),
        (".webp", "webp"),
        (".avif", "avif"),
        (".gif", "gif"),
    ] {
        if lower.contains(needle) {
            return Some(ext);
        }
    }
    None
}

/* =========================
 * LLM call + JSON extract
 * ========================= */

fn extract_json_object(s: &str) -> Option<String> {
    static FENCE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)```json\s*(\{.*?\})\s*```").unwrap());
    if let Some(c) = FENCE.captures(s) {
        return Some(c[1].to_string());
    }

    // Fallback: largest balanced {...}
    let mut best: Option<(usize, usize)> = None;
    let mut depth = 0usize;
    let mut start = None;

    for (i, ch) in s.char_indices() {
        match ch {
            '{' => {
                depth += 1;
                if depth == 1 {
                    start = Some(i);
                }
            }
            '}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(st) = start {
                            let cand = (st, i);
                            if best
                                .map(|(a, b)| (b - a) < (cand.1 - cand.0))
                                .unwrap_or(true)
                            {
                                best = Some(cand);
                            }
                        }
                        start = None;
                    }
                }
            }
            _ => {}
        }
    }
    best.map(|(a, b)| s[a..=b].to_string())
}

pub async fn call_llm_json(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    model: &str,
    system: &str,
    user: &str,
) -> Result<JsonValue, String> {
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));

    #[derive(Serialize)]
    struct Msg<'a> {
        role: &'a str,
        content: &'a str,
    }
    #[derive(Serialize)]
    struct Body<'a> {
        model: &'a str,
        messages: Vec<Msg<'a>>,
        temperature: f32,
        #[serde(skip_serializing_if = "Option::is_none")]
        response_format: Option<JsonValue>,
    }

    let body = Body {
        model,
        messages: vec![
            Msg {
                role: "system",
                content: system,
            },
            Msg {
                role: "user",
                content: user,
            },
        ],
        temperature: 0.1,
        response_format: Some(json!({ "type": "json_object" })), // OpenAI/OpenRouter-style
    };

    let resp = client
        .post(&url)
        .bearer_auth(token)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .timeout(Duration::from_secs(120))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("sending LLM request: {e}"))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if status != reqwest::StatusCode::OK {
        return Err(format!("LLM HTTP {}: {}", status, text));
    }

    let envelope: JsonValue =
        serde_json::from_str(&text).map_err(|e| format!("decoding LLM envelope: {e}"))?;

    let content = envelope
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .or_else(|| {
            envelope
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c0| c0.get("text"))
                .and_then(|v| v.as_str())
        })
        .ok_or_else(|| "LLM response missing content".to_string())?;

    if let Ok(js) = serde_json::from_str::<JsonValue>(content) {
        return Ok(js);
    }

    if let Some(json_s) = extract_json_object(content) {
        let js = serde_json::from_str::<JsonValue>(&json_s)
            .map_err(|e| format!("parsing extracted JSON: {e}"))?;
        return Ok(js);
    }

    let preview = if content.len() > 500 {
        &content[..500]
    } else {
        content
    };
    Err(format!(
        "LLM did not return valid JSON. Preview: {}",
        preview
    ))
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
    fn from_json(v: &JsonValue) -> Result<Self, String> {
        let title = v
            .get("title")
            .and_then(|x| x.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty());

        Ok(Self {
            title,
            ingredients: v.get("ingredients").cloned().unwrap_or(JsonValue::Null),
            instructions: v.get("instructions").cloned().unwrap_or(JsonValue::Null),
        })
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
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
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
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
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
                (v.round() as i64).to_string()
            } else if u == "kg" || u == "L" {
                trim_zeros(format!("{:.2}", v))
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
