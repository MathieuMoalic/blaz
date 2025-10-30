use crate::{
    error::AppResult,
    models::{AppState, Ingredient, Recipe, RecipeRow},
    routes::{parse_recipe_image::extract_main_image_url, recipes::fetch_and_store_recipe_image},
};
use anyhow::{Result, anyhow};
use axum::{Json, extract::State, http::StatusCode};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use scraper::{ElementRef, Html, Node, Selector};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::time::Duration;
use tracing::error;

#[derive(Deserialize)]
pub struct ImportReq {
    pub url: String,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Extracted {
    ingredients: Vec<Ingredient>,
    instructions: Vec<String>,
}

/* ---------- Route ---------- */

pub async fn import_from_url(
    State(state): State<AppState>,
    Json(req): Json<ImportReq>,
) -> AppResult<Json<Recipe>> {
    // CHANGED
    // 1) Fetch HTML
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(95))
        .connect_timeout(Duration::from_secs(10))
        .pool_idle_timeout(Some(Duration::from_secs(30)))
        .build()
        .map_err(|e| {
            error!(?e, "reqwest client build failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let html = client
        .get(&req.url)
        .header(USER_AGENT, "blaz/recipe-importer")
        .send()
        .await
        .map_err(|e| {
            error!(?e, url = %req.url, "fetch failed");
            StatusCode::BAD_GATEWAY
        })?
        .text()
        .await
        .map_err(|e| {
            error!(?e, url = %req.url, "read body failed");
            StatusCode::BAD_GATEWAY
        })?;

    // 2) Visible text + title
    let text = extract_visible_text(&html);
    let title = extract_title(&html).unwrap_or_else(|| "Imported recipe".to_string());

    // 3) Call HF router
    let base = std::env::var("BLAZ_LLM_API_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "https://router.huggingface.co/v1".into());
    let token = std::env::var("BLAZ_LLM_API_KEY").unwrap_or_default();
    let model = req
        .model
        .or_else(|| std::env::var("BLAZ_LLM_MODEL").ok())
        .unwrap_or_else(|| "meta-llama/Llama-3.1-8B-Instruct".into());

    let system = build_instructions();
    let extracted = match call_llm(&client, &base, &token, &model, system, &text).await {
        Ok(ok) => ok,
        Err(e) => {
            error!(?e, "llm extract failed");
            return Err(StatusCode::BAD_GATEWAY.into());
        }
    };
    let main_img_url = extract_main_image_url(&html, &req.url);

    // 4) Insert into DB (no image yet)
    let ingredients_json = serde_json::to_string(&extracted.ingredients).unwrap_or("[]".into());
    let instructions_json =
        serde_json::to_string(&extracted.instructions).unwrap_or_else(|_| "[]".into());

    let yield_text = String::new();
    let notes_text = String::new();

    let row: RecipeRow = sqlx::query_as::<_, RecipeRow>(
    r#"
    INSERT INTO recipes (title, source, "yield", notes, ingredients, instructions, created_at, updated_at)
    VALUES (?, ?, ?, ?, json(?), json(?), CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
    RETURNING id, title, source, "yield", notes,
              created_at, updated_at,
              ingredients, instructions,
              image_path_small, image_path_full,
              macros
    "#,)
        .bind(title)
        .bind(&req.url)
        .bind(&yield_text)
        .bind(&notes_text)
        .bind(ingredients_json)
        .bind(instructions_json)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            error!(?e, url = %req.url, "insert imported recipe failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // 5) If we found a main image, fetch/convert/store and UPDATE the row
    if let Some(img_url) = main_img_url {
        match fetch_and_store_recipe_image(&client, &img_url, &state, row.id).await {
            Ok((full, thumb)) => {
                if let Err(e) = sqlx::query(
                    r#"
                UPDATE recipes
                   SET image_path_full  = ?,
                       image_path_small = ?,
                       updated_at       = CURRENT_TIMESTAMP
                 WHERE id = ?
                "#,
                )
                .bind(&full)
                .bind(&thumb)
                .bind(row.id)
                .execute(&state.pool)
                .await
                {
                    tracing::warn!(?e, %img_url, %row.id, "failed to update recipe image paths");
                }
            }
            Err(e) => {
                tracing::warn!(?e, %img_url, %row.id, "failed to fetch/convert main image");
            }
        }
    }

    // 6) Return the fresh row (re-select so we include image paths if they were set)
    let final_row: RecipeRow = sqlx::query_as::<_, RecipeRow>(
        r#"
    SELECT id, title, source, "yield", notes,
           created_at, updated_at,
           ingredients, instructions,
           image_path_small, image_path_full,
           macros
      FROM recipes
     WHERE id = ?
    "#,
    )
    .bind(row.id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        error!(?e, id = row.id, "refetch after image update failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(final_row.into()))
}
// 5) If we found a main image, fetch/convert/store and UPDATE the row
/* ---------- System prompt ---------- */

fn build_instructions() -> String {
    r#"You are a precise recipe data extractor and normalizer.

INPUT: plain text from a recipe page (any language).
OUTPUT: STRICT JSON with exactly these keys:
{"ingredients":[{"quantity":null|number,"unit":null|"g"|"kg"|"ml"|"L"|"tsp"|"tbsp","name":string}], "instructions":[]}

TASK:
- Translate to English.
- Convert ALL imperial units to metric in the INGREDIENTS.
  * Allowed units in ingredients: g, kg, ml, L, tsp, tbsp.
  * Never use: cup, cups, oz, ounce, ounces, fl oz, pound, lb.
  * Keep tsp and tbsp abbreviations as written (do not spell out).
- Keep amounts as numbers; keep ranges by converting both ends (e.g., 2–4 cups → 480–960 ml).
- For solid items, convert oz→g (1 oz ≈ 28 g). For liquids, convert fl oz→ml (1 fl oz ≈ 30 ml). For cups→ml (1 cup ≈ 240 ml).
- If an ingredient has prep words (e.g., sliced, diced, minced), put them AFTER the ingredient name, separated by ", " (comma + space).
  Example: "2 carrots, diced".
- If data is missing, return an empty array for that key.
- Do NOT include commentary or extra keys.
- When a quantity is a range, replace the range with the mean value of the range.
- Round quantities sensibly.
- Use 0.5/0.25/0.75 style; never 1/2, 1/4, etc.
- If no numeric quantity, set "quantity": null and "unit": null, keep the name.
- instructions: array of steps (strings). No commentary.

FORMAT EXAMPLES:
{"ingredients":["2 cloves garlic, minced","150 g flour","2 carrots, diced"],"instructions":["Cook the garlic.","Fold in flour."]}

SELF-CHECK:
Before answering, verify no banned units appear in INGREDIENTS. If any do, fix them and re-check. Answer only with the final JSON.
"#.to_string()
}

/* ---------- Visible-text extraction ---------- */

fn extract_visible_text(html: &str) -> String {
    let doc = Html::parse_document(html);
    let root = Selector::parse("body")
        .ok()
        .and_then(|s| doc.select(&s).next())
        .unwrap_or_else(|| doc.root_element());

    let mut out = String::new();
    collect_text(root, &mut out);
    normalize_blocks(&out)
}

fn collect_text(el: ElementRef<'_>, out: &mut String) {
    let name = el.value().name();
    if is_blacklisted_tag(name)
        || el.value().attr("hidden").is_some()
        || el.value().attr("aria-hidden") == Some("true")
    {
        return;
    }
    if is_block(name) && !out.ends_with('\n') {
        out.push('\n');
    }
    for child in el.children() {
        match child.value() {
            Node::Text(t) => {
                let text = t.clone().to_string();
                if text.trim().is_empty() {
                    continue;
                }
                if !out.ends_with(|c: char| c.is_whitespace() || c == '\n')
                    && !text.starts_with(char::is_whitespace)
                {
                    out.push(' ');
                }
                out.push_str(&text);
            }
            Node::Element(_) => {
                if let Some(child_el) = ElementRef::wrap(child) {
                    collect_text(child_el, out);
                }
            }
            _ => {}
        }
    }
    if name == "br" {
        out.push('\n');
    }
    if is_block(name) && !out.ends_with('\n') {
        out.push('\n');
    }
}

fn is_blacklisted_tag(name: &str) -> bool {
    matches!(
        name,
        "script"
            | "style"
            | "noscript"
            | "template"
            | "svg"
            | "math"
            | "head"
            | "link"
            | "meta"
            | "nav"
            | "aside"
            | "footer"
            | "form"
            | "iframe"
            | "canvas"
            | "video"
            | "audio"
            | "picture"
            | "source"
            | "track"
            | "object"
            | "embed"
            | "button"
            | "label"
            | "input"
            | "select"
            | "textarea"
    )
}
fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "section"
            | "article"
            | "main"
            | "header"
            | "footer"
            | "ul"
            | "ol"
            | "li"
            | "table"
            | "thead"
            | "tbody"
            | "tr"
            | "td"
            | "th"
            | "figure"
            | "figcaption"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "pre"
            | "blockquote"
    )
}
fn normalize_blocks(s: &str) -> String {
    let mut lines: Vec<String> = s
        .lines()
        .map(collapse_ws)
        .filter(|l| !l.is_empty())
        .collect();
    lines.dedup();
    lines.join("\n")
}
fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !ws {
                out.push(' ');
                ws = true;
            }
        } else {
            ws = false;
            out.push(ch);
        }
    }
    out.trim().to_string()
}

/* ---------- Title helper ---------- */

fn extract_title(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    // Prefer <meta property="og:title">, fallback to <title>, then first <h1>
    if let Ok(sel) = Selector::parse(r#"meta[property="og:title"]"#) {
        if let Some(el) = doc.select(&sel).next() {
            if let Some(c) = el.value().attr("content") {
                let t = c.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }
    if let Ok(sel) = Selector::parse("title") {
        if let Some(el) = doc.select(&sel).next() {
            let t = el.text().collect::<String>().trim().to_string();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    if let Ok(sel) = Selector::parse("h1") {
        if let Some(el) = doc.select(&sel).next() {
            let t = el.text().collect::<String>().trim().to_string();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

/* ---------- HF call (OpenAI-compatible) ---------- */

async fn call_llm(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    model: &str,
    system: String,
    user_text: &str,
) -> Result<Extracted> {
    let body = json!({
        "model": model,
        "messages": [
            {"role":"system","content": system},
            {"role":"user","content": format!("TEXT:\n{}", user_text)}
        ],
        "temperature": 0.1,
        "max_tokens": 900,
        "response_format": { "type": "json_object" }
    });

    let mut req = client
        .post(format!("{}/chat/completions", base))
        .header(CONTENT_TYPE, "application/json");

    if !token.is_empty() {
        req = req.header(AUTHORIZATION, format!("Bearer {}", token));
    }

    let resp = req.json(&body).send().await?;
    let status = resp.status();
    let body_text = tokio::time::timeout(Duration::from_secs(80), resp.text())
        .await
        .map_err(|_| anyhow::anyhow!("LLM response read timed out"))??;

    if !status.is_success() {
        anyhow::bail!("llm router {}: {}", status, body_text);
    }

    #[derive(Deserialize)]
    struct ChoiceMsg {
        content: String,
    }
    #[derive(Deserialize)]
    struct Choice {
        message: ChoiceMsg,
    }
    #[derive(Deserialize)]
    struct ChatResp {
        choices: Vec<Choice>,
    }

    let parsed: ChatResp = serde_json::from_str(&body_text)?;
    let content = parsed
        .choices
        .first()
        .ok_or_else(|| anyhow!("no choices"))?
        .message
        .content
        .trim()
        .to_string();

    // Parse JSON (models may still wrap; grab first {…} if needed)
    if let Ok(ok) = serde_json::from_str::<Extracted>(&content) {
        return Ok(normalize_output(ok));
    }
    if let Some(obj) = find_first_json_object(&content) {
        let ok: Extracted = serde_json::from_str(&obj)?;
        return Ok(normalize_output(ok));
    }
    Err(anyhow!("model did not return JSON: {}", content))
}

fn find_first_json_object(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut start = None;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\\' if in_str => {
                esc = !esc;
                continue;
            }
            b'"' if !esc => {
                in_str = !in_str;
            }
            b'{' if !in_str => {
                if start.is_none() {
                    start = Some(i)
                }
                depth += 1;
            }
            b'}' if !in_str && depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let st = start?;
                    return Some(s[st..=i].to_string());
                }
            }
            _ => {
                esc = false;
            }
        }
    }
    None
}

fn normalize_output(mut e: Extracted) -> Extracted {
    // dedupe ingredients by (unit|lowercased name); ignore quantity to avoid
    // “same item with slightly different amounts” duplicates
    let mut i_seen = HashSet::new();
    e.ingredients.retain(|ing| i_seen.insert(norm_ing_key(ing)));

    let mut s_seen = HashSet::new();
    e.instructions.retain(|s| s_seen.insert(norm_key(s)));

    e
}

// case-insensitive line key for instructions
fn norm_key(s: &str) -> String {
    s.trim().to_lowercase()
}

// normalized ingredient key (ignore quantity)
fn norm_ing_key(ing: &Ingredient) -> String {
    format!(
        "{}|{}",
        ing.unit.as_deref().unwrap_or("").to_lowercase(),
        ing.name.trim().to_lowercase()
    )
}
