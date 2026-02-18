use crate::categories::{Category, guess_category};
use crate::error::AppError;
use axum::http::StatusCode;
use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite};

use crate::error::AppResult;
use crate::models::{AppState, NewItem, ShoppingItemView};
use crate::units::{canon_unit_str, normalize_name, to_canonical_qty_unit};

fn internal_err<E: std::error::Error>(err: E) -> AppError {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into()
}

fn patch_update_err(err: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(db) = &err {
        if db.is_unique_violation() {
            return (
                StatusCode::CONFLICT,
                "shopping item with the same name/unit already exists".into(),
            )
                .into();
        }
    }
    internal_err(err)
}

/* ---------- Request/response types ---------- */

#[derive(Deserialize, Debug)]
pub struct UpdateShoppingItem {
    pub done: Option<bool>,
    pub category: Option<String>,

    /// Backwards-compatible free-form update.
    /// If provided, it takes priority over name/unit/quantity fields.
    pub text: Option<String>,

    /// Structured edits:
    pub name: Option<String>,
    pub unit: Option<String>,
    pub quantity: Option<f64>,
}

#[derive(Deserialize, Clone)]
pub struct InIngredient {
    pub quantity: Option<f64>,
    pub unit: Option<String>, // "g","kg","ml","L","tsp","tbsp" or null
    pub name: String,
    pub category: Option<String>,
}

#[derive(Deserialize)]
pub struct MergeReq {
    pub items: Vec<InIngredient>,
    pub recipe_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ParsedItem {
    pub qty: Option<f64>,
    pub unit: Option<String>, // normalized short unit, e.g. "g","kg","ml","L","tsp","tbsp"
    pub name_raw: String,     // as extracted from the line
    pub name_norm: String,    // normalized for merge key/category
}

/* ---------- LLM normalization types ---------- */

#[derive(Serialize)]
struct NormalizeLlmRequest {
    model: String,
    messages: Vec<NormalizeLlmMessage>,
    max_tokens: usize,
    temperature: f32,
}

#[derive(Serialize)]
struct NormalizeLlmMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct NormalizeLlmResponse {
    choices: Vec<NormalizeLlmChoice>,
}

#[derive(Deserialize)]
struct NormalizeLlmChoice {
    message: NormalizeLlmMessageContent,
}

#[derive(Deserialize)]
struct NormalizeLlmMessageContent {
    content: String,
}

fn parse_qty_token(t: &str) -> Option<f64> {
    let t = t.trim().replace(',', ".");
    if t.is_empty() {
        return None;
    }

    if let Some((a, b)) = t.split_once('-').or_else(|| t.split_once('–')) {
        let x = a.trim().parse::<f64>().ok()?;
        let y = b.trim().parse::<f64>().ok()?;
        return Some((x + y) / 2.0);
    }

    t.parse::<f64>().ok()
}

fn normalize_unit_token(t: &str) -> Option<String> {
    let u = t.trim();
    if u.is_empty() {
        return None;
    }
    canon_unit_str(u).map(std::string::ToString::to_string)
}

fn create_plain_name_item(raw: &str, reason: &str) -> ParsedItem {
    let name_raw = raw.to_string();
    let name_norm = normalize_name(&name_raw);
    let parsed = ParsedItem {
        qty: None,
        unit: None,
        name_raw,
        name_norm,
    };

    tracing::info!(
        raw = %raw,
        qty = ?parsed.qty,
        unit = ?parsed.unit,
        name_raw = %parsed.name_raw,
        name_norm = %parsed.name_norm,
        "parsed ingredient line ({reason})"
    );

    parsed
}

/// Parse a line that may look like:
/// - "120 g flour"
/// - "2-3 apples"
/// - "1 banana"
/// - "milk"
///
/// The function is intentionally tolerant:
/// - If it doesn’t start with a number, qty/unit are None and the whole line is the name.
/// - If it starts with a number but the remaining name is empty, it falls back to treating
///   the whole line as the name.
fn parse_item_line(raw: &str) -> Option<ParsedItem> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    let tokens: Vec<&str> = raw.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    // Try parse leading qty
    let qty = parse_qty_token(tokens[0]);

    // If no leading number, treat whole line as plain name
    if qty.is_none() {
        return Some(create_plain_name_item(raw, "no leading quantity"));
    }

    // Optional unit
    let mut idx = 1usize;
    let mut unit: Option<String> = None;

    if let Some(t1) = tokens.get(1) {
        if let Some(un) = normalize_unit_token(t1) {
            unit = Some(un);
            idx = 2;
        }
    }

    // Optional "of"
    if tokens.get(idx).copied() == Some("of") {
        idx += 1;
    }

    // Remaining tokens are the name
    if idx >= tokens.len() {
        // Mirror old fallback: ignore parsed qty/unit if name is missing
        return Some(create_plain_name_item(raw, "missing name after qty"));
    }

    let name_raw = tokens[idx..].join(" ");
    let name_norm = normalize_name(&name_raw);

    let parsed = ParsedItem {
        qty,
        unit,
        name_raw,
        name_norm,
    };

    tracing::info!(
        raw = %raw,
        qty = ?parsed.qty,
        unit = ?parsed.unit,
        name_raw = %parsed.name_raw,
        name_norm = %parsed.name_norm,
        "parsed ingredient line"
    );

    Some(parsed)
}

/* ---------- DB helpers ---------- */

async fn fetch_view_by_id(state: &AppState, id: i64) -> Result<ShoppingItemView, sqlx::Error> {
    sqlx::query_as::<_, ShoppingItemView>(
        r"
        SELECT id, text, done, category, recipe_ids, recipe_titles
          FROM shopping_items_view
         WHERE id = ?
        ",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
}

#[derive(sqlx::FromRow)]
struct ShoppingItemRow {
    name: String,
    unit: Option<String>,
    quantity: Option<f64>,
}

async fn fetch_raw_by_id(state: &AppState, id: i64) -> Result<ShoppingItemRow, sqlx::Error> {
    sqlx::query_as::<_, ShoppingItemRow>(
        r"
        SELECT name, unit, quantity
          FROM shopping_items
         WHERE id = ?
        ",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
}

/// Unique key used for merging rows: "<unit>|<name>" with normalized name/unit.
/// For unit-less items the key starts with a leading pipe: "|<name>".
fn make_key(name_norm: &str, unit_norm: Option<&str>) -> String {
    match unit_norm {
        Some(u) if !u.is_empty() => format!("{u}|{name_norm}"),
        _ => format!("|{name_norm}"),
    }
}

/// Normalize a single ingredient name with caching.
/// Checks cache first, then LLM, then saves result.
async fn normalize_ingredient_name(state: &AppState, raw_name: &str) -> String {
    let raw_lower = raw_name.trim().to_lowercase();
    
    // 1. Check cache first
    if let Ok(Some(cached)) = sqlx::query_scalar::<_, String>(
        "SELECT normalized_name FROM ingredient_normalizations WHERE raw_name = ?"
    )
    .bind(&raw_lower)
    .fetch_optional(&state.pool)
    .await
    {
        tracing::debug!(raw = %raw_name, cached = %cached, "Using cached normalization");
        return cached;
    }

    // 2. Call LLM if configured
    let normalized = if state.config.llm_api_key.is_some() {
        call_llm_normalize_single(state, &raw_lower).await
            .unwrap_or_else(|| normalize_name(&raw_lower))
    } else {
        normalize_name(&raw_lower)
    };

    // 3. Save to cache
    let _ = sqlx::query(
        "INSERT OR REPLACE INTO ingredient_normalizations (raw_name, normalized_name) VALUES (?, ?)"
    )
    .bind(&raw_lower)
    .bind(&normalized)
    .execute(&state.pool)
    .await;

    tracing::info!(raw = %raw_name, normalized = %normalized, "Normalized ingredient name");
    normalized
}

/// Normalize multiple ingredient names in one LLM call (batch).
/// Checks cache for each, batches uncached ones, saves results.
async fn normalize_ingredient_names_batch(state: &AppState, raw_names: &[String]) -> Vec<String> {
    let mut results = Vec::with_capacity(raw_names.len());
    let mut uncached_indices = Vec::new();
    let mut uncached_names = Vec::new();

    // 1. Check cache for each name
    for (idx, raw_name) in raw_names.iter().enumerate() {
        let raw_lower = raw_name.trim().to_lowercase();
        
        if let Ok(Some(cached)) = sqlx::query_scalar::<_, String>(
            "SELECT normalized_name FROM ingredient_normalizations WHERE raw_name = ?"
        )
        .bind(&raw_lower)
        .fetch_optional(&state.pool)
        .await
        {
            results.push(cached);
        } else {
            results.push(String::new()); // Placeholder
            uncached_indices.push(idx);
            uncached_names.push(raw_lower);
        }
    }

    // 2. If all cached, return early
    if uncached_names.is_empty() {
        tracing::debug!("All {} names found in cache", raw_names.len());
        return results;
    }

    tracing::info!(
        total = raw_names.len(),
        cached = raw_names.len() - uncached_names.len(),
        uncached = uncached_names.len(),
        "Batch normalizing ingredients"
    );

    // 3. Batch call LLM for uncached names
    let normalized_uncached = if state.config.llm_api_key.is_some() {
        call_llm_normalize_batch(state, &uncached_names).await
            .unwrap_or_else(|| uncached_names.iter().map(|n| normalize_name(n)).collect())
    } else {
        uncached_names.iter().map(|n| normalize_name(n)).collect()
    };

    // 4. Fill in results and save to cache
    for (i, &result_idx) in uncached_indices.iter().enumerate() {
        let normalized = &normalized_uncached[i];
        results[result_idx].clone_from(normalized);
        
        // Save to cache
        let _ = sqlx::query(
            "INSERT OR REPLACE INTO ingredient_normalizations (raw_name, normalized_name) VALUES (?, ?)"
        )
        .bind(&uncached_names[i])
        .bind(normalized)
        .execute(&state.pool)
        .await;
    }

    results
}

/// Strip surrounding quotes from a string if present.
fn strip_quotes(s: &str) -> String {
    let trimmed = s.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

/// Call LLM to normalize a single ingredient name.
async fn call_llm_normalize_single(state: &AppState, raw_name: &str) -> Option<String> {
    let request = NormalizeLlmRequest {
        model: state.config.llm_model.clone(),
        messages: vec![
            NormalizeLlmMessage {
                role: "system".to_string(),
                content: state.config.system_prompt_normalize.clone(),
            },
            NormalizeLlmMessage {
                role: "user".to_string(),
                content: raw_name.to_string(),
            },
        ],
        max_tokens: 50,
        temperature: 0.0,
    };

    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", state.config.llm_api_url);

    match client
        .post(&url)
        .header("Content-Type", "application/json")
        .header(
            "Authorization",
            format!("Bearer {}", state.config.llm_api_key.as_ref()?),
        )
        .json(&request)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<NormalizeLlmResponse>().await {
                Ok(llm_resp) => llm_resp
                    .choices
                    .first()
                    .map(|c| strip_quotes(&c.message.content).to_lowercase()),
                Err(e) => {
                    tracing::warn!("Failed to parse LLM response: {}", e);
                    None
                }
            }
        }
        Ok(resp) => {
            tracing::warn!("LLM returned error status: {}", resp.status());
            None
        }
        Err(e) => {
            tracing::warn!("Failed to call LLM: {}", e);
            None
        }
    }
}

/// Call LLM to normalize multiple ingredient names in one request.
async fn call_llm_normalize_batch(state: &AppState, raw_names: &[String]) -> Option<Vec<String>> {
    let batch_input = serde_json::to_string(raw_names).ok()?;
    
    let request = NormalizeLlmRequest {
        model: state.config.llm_model.clone(),
        messages: vec![
            NormalizeLlmMessage {
                role: "system".to_string(),
                content: state.config.system_prompt_normalize.clone(),
            },
            NormalizeLlmMessage {
                role: "user".to_string(),
                content: batch_input,
            },
        ],
        max_tokens: raw_names.len() * 20, // ~20 tokens per ingredient
        temperature: 0.0,
    };

    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", state.config.llm_api_url);

    match client
        .post(&url)
        .header("Content-Type", "application/json")
        .header(
            "Authorization",
            format!("Bearer {}", state.config.llm_api_key.as_ref()?),
        )
        .json(&request)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<NormalizeLlmResponse>().await {
                Ok(llm_resp) => llm_resp
                    .choices
                    .first()
                    .and_then(|choice| {
                        serde_json::from_str::<Vec<String>>(&choice.message.content)
                            .ok()
                            .map(|vec| {
                                vec.into_iter()
                                    .map(|s| strip_quotes(&s).to_lowercase())
                                    .collect()
                            })
                    }),
                Err(e) => {
                    tracing::warn!("Failed to parse LLM batch response: {}", e);
                    None
                }
            }
        }
        Ok(resp) => {
            tracing::warn!("LLM batch returned error status: {}", resp.status());
            None
        }
        Err(e) => {
            tracing::warn!("Failed to call LLM batch: {}", e);
            None
        }
    }
}

/* ---------- Routes ---------- */

/// GET /shopping
///
/// Returns ONLY non-done items.
/// Done items are kept in DB so their unit/category data remains for future edits.
///
/// # Errors
/// Err if querying the database fails.
pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<ShoppingItemView>>> {
    let mut rows = sqlx::query_as::<_, ShoppingItemView>(
        r"
        SELECT id, text, done, category, recipe_ids, recipe_titles
          FROM shopping_items_view
         WHERE done = 0
         ORDER BY id
        ",
    )
    .fetch_all(&state.pool)
    .await?;

    // Optional nicer ordering: category order (enum order), then id.
    rows.sort_by_key(|r| {
        let cat_key = r
            .category
            .as_deref()
            .and_then(Category::from_str)
            .map_or(255u8, Category::sort_key);
        (cat_key, r.id)
    });

    Ok(Json(rows))
}

/// GET /shopping/all-texts
///
/// Returns all unique item texts (including done items) for autocomplete.
///
/// # Errors
/// Err if querying the database fails.
pub async fn list_all_texts(State(state): State<AppState>) -> AppResult<Json<Vec<String>>> {
    let texts: Vec<String> = sqlx::query_scalar(
        r"
        SELECT DISTINCT text
          FROM shopping_items_view
         ORDER BY text
        ",
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(texts))
}

/// POST /shopping
///
/// # Errors
/// Err if the input text is empty.
/// Err if inserting or fetching the shopping item fails.
pub async fn create(
    State(state): State<AppState>,
    Json(new): Json<NewItem>,
) -> AppResult<Json<ShoppingItemView>> {
    let text = new.text.trim();
    if text.is_empty() {
        return Err(StatusCode::BAD_REQUEST.into());
    }

    let parsed = parse_item_line(text).ok_or(StatusCode::BAD_REQUEST)?;

    // Structured path only if a leading qty was detected
    if parsed.qty.is_some() {
        let (mut unit_norm, qty_norm) = to_canonical_qty_unit(parsed.unit.as_deref(), parsed.qty);
        if qty_norm.is_none() {
            unit_norm = None;
        }

        // Use LLM normalization for better merging
        let name_normalized = normalize_ingredient_name(&state, &parsed.name_raw).await;
        let key = make_key(&name_normalized, unit_norm);

        // Reuse existing category if present to avoid redundant LLM calls.
        let existing: Option<(i64, Option<String>, i64)> =
            sqlx::query_as(r"SELECT id, category, done FROM shopping_items WHERE key = ?")
                .bind(&key)
                .fetch_optional(&state.pool)
                .await?;

        let category_guess = match existing.as_ref().and_then(|(_, c, _)| c.clone()) {
            Some(c) if !c.trim().is_empty() => c,
            _ => guess_category(&state, &parsed.name_raw).await,
        };

        sqlx::query(
            r"
            INSERT INTO shopping_items (name, unit, quantity, done, key, category)
            VALUES (?, ?, ?, 0, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
              quantity = CASE 
                WHEN shopping_items.done = 1 THEN excluded.quantity
                ELSE COALESCE(shopping_items.quantity, 0) + COALESCE(excluded.quantity, 0)
              END,
              category = COALESCE(shopping_items.category, excluded.category),
              name = excluded.name,
              unit = excluded.unit,
              done = 0
            ",
        )
        .bind(&name_normalized)
        .bind(unit_norm)
        .bind(qty_norm)
        .bind(&key)
        .bind(&category_guess)
        .execute(&state.pool)
        .await?;

        let (id,): (i64,) = sqlx::query_as("SELECT id FROM shopping_items WHERE key = ?")
            .bind(&key)
            .fetch_one(&state.pool)
            .await?;

        let row = fetch_view_by_id(&state, id).await?;
        return Ok(Json(row));
    }

    // Fallback: unitless item - also use normalization
    let name_normalized = normalize_ingredient_name(&state, &parsed.name_raw).await;
    let key = make_key(&name_normalized, None);

    // Reuse existing category if present to avoid redundant LLM calls.
    let existing_cat: Option<String> =
        sqlx::query_scalar(r"SELECT category FROM shopping_items WHERE key = ?")
            .bind(&key)
            .fetch_optional(&state.pool)
            .await?;

    let category_guess = match existing_cat {
        Some(c) if !c.trim().is_empty() => c,
        _ => guess_category(&state, &parsed.name_raw).await,
    };

    sqlx::query(
        r"
        INSERT INTO shopping_items (name, unit, quantity, done, key, category)
        VALUES (?, NULL, NULL, 0, ?, ?)
        ON CONFLICT(key) DO UPDATE SET
          category = COALESCE(shopping_items.category, excluded.category),
          name = excluded.name,
          done = 0
        ",
    )
    .bind(&name_normalized)
    .bind(&key)
    .bind(&category_guess)
    .execute(&state.pool)
    .await?;

    let (id,): (i64,) = sqlx::query_as("SELECT id FROM shopping_items WHERE key = ?")
        .bind(&key)
        .fetch_one(&state.pool)
        .await?;

    let row = fetch_view_by_id(&state, id).await?;
    Ok(Json(row))
}

/* ---------- PATCH helpers ---------- */

fn push_sep(qb: &mut QueryBuilder<Sqlite>, wrote: &mut bool) {
    if *wrote {
        qb.push(", ");
    } else {
        *wrote = true;
    }
}

fn apply_done_update(qb: &mut QueryBuilder<Sqlite>, wrote: &mut bool, done: Option<bool>) {
    if let Some(d) = done {
        push_sep(qb, wrote);
        qb.push("done = ");
        qb.push_bind(i64::from(d));
        
        // Clear recipe_ids when marking as done so list resets
        if d {
            push_sep(qb, wrote);
            qb.push("recipe_ids = '[]'");
        }
    }
}

fn apply_category_update(
    qb: &mut QueryBuilder<Sqlite>,
    wrote: &mut bool,
    category: Option<String>,
) -> AppResult<()> {
    let Some(mut cat) = category else {
        return Ok(());
    };

    push_sep(qb, wrote);

    cat = cat.trim().to_string();
    if cat.is_empty() {
        qb.push("category = NULL");
        return Ok(());
    }

    if Category::from_str(&cat).is_none() {
        return Err((StatusCode::BAD_REQUEST, "invalid category".into()).into());
    }

    qb.push("category = ");
    qb.push_bind(cat);

    Ok(())
}

async fn apply_text_update(
    qb: &mut QueryBuilder<'_, Sqlite>,
    wrote: &mut bool,
    state: &AppState,
    payload: &UpdateShoppingItem,
) -> AppResult<bool> {
    let Some(t) = payload.text.as_deref() else {
        return Ok(false);
    };

    let parsed =
        parse_item_line(t).ok_or_else(|| (StatusCode::BAD_REQUEST, "empty text".into()))?;

    let (mut unit_norm, qty_norm) = to_canonical_qty_unit(parsed.unit.as_deref(), parsed.qty);
    if qty_norm.is_none() {
        unit_norm = None;
    }

    let key = make_key(&parsed.name_norm, unit_norm);

    push_sep(qb, wrote);

    qb.push("name = ");
    qb.push_bind(parsed.name_norm);

    qb.push(", quantity = ");
    if let Some(q) = qty_norm {
        qb.push_bind(q);
    } else {
        qb.push("NULL");
    }

    qb.push(", unit = ");
    if let Some(u) = unit_norm {
        qb.push_bind(u);
    } else {
        qb.push("NULL");
    }

    qb.push(", key = ");
    qb.push_bind(key);

    // If `category` was NOT explicitly provided, refresh it based on the name.
    if payload.category.is_none() {
        let cat_guess = guess_category(state, &parsed.name_raw).await;
        qb.push(", category = ");
        qb.push_bind(cat_guess);
    }

    Ok(true)
}

async fn apply_structured_update(
    qb: &mut QueryBuilder<'_, Sqlite>,
    wrote: &mut bool,
    state: &AppState,
    id: i64,
    payload: &UpdateShoppingItem,
) -> AppResult<bool> {
    let has_structured =
        payload.name.is_some() || payload.unit.is_some() || payload.quantity.is_some();

    if !has_structured {
        return Ok(false);
    }

    let current = fetch_raw_by_id(state, id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let new_name_raw = if let Some(n) = payload.name.clone() {
        let n = n.trim().to_string();
        if n.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "empty name".into()).into());
        }
        n
    } else {
        current.name.clone()
    };

    let new_name_norm = normalize_name(&new_name_raw);

    let new_unit_raw = payload.unit.clone().map(|u| u.trim().to_string());
    let new_unit_raw = match new_unit_raw.as_deref() {
        Some("") => None, // allow clearing
        Some(u) => Some(u.to_string()),
        None => current.unit.clone(),
    };

    let new_qty = payload.quantity.or(current.quantity);

    let (mut unit_norm, qty_norm) = to_canonical_qty_unit(new_unit_raw.as_deref(), new_qty);
    if qty_norm.is_none() {
        unit_norm = None;
    }

    let key = make_key(&new_name_norm, unit_norm);

    push_sep(qb, wrote);

    qb.push("name = ");
    qb.push_bind(new_name_norm);

    qb.push(", quantity = ");
    if let Some(q) = qty_norm {
        qb.push_bind(q);
    } else {
        qb.push("NULL");
    }

    qb.push(", unit = ");
    if let Some(u) = unit_norm {
        qb.push_bind(u);
    } else {
        qb.push("NULL");
    }

    qb.push(", key = ");
    qb.push_bind(key);

    // Auto-guess category only if:
    // - `category` wasn't explicitly provided
    // - and `name` was part of this patch
    if payload.category.is_none() && payload.name.is_some() {
        let cat_guess = guess_category(state, &new_name_raw).await;
        qb.push(", category = ");
        qb.push_bind(cat_guess);
    }

    Ok(true)
}

/* ---------- Route ---------- */

/// PATCH `/shopping/{id}`
///
/// Supports updates to:
/// - `done`
/// - `category`
/// - `text` (free-form; re-parses qty/unit/name; takes priority)
/// - `name`, `unit`, `quantity` (structured)
///
/// Done items remain in DB; `list()` simply hides them.
///
/// # Errors
/// - Returns `400` if `text`/`name` is empty or if `category` is invalid.
/// - Returns `409` on `key` conflict.
/// - Returns `404` if the item does not exist.
/// - Returns `500` on unexpected database errors.
pub async fn patch_shopping_item(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateShoppingItem>,
) -> AppResult<Json<ShoppingItemView>> {
    let mut qb = QueryBuilder::<Sqlite>::new("UPDATE shopping_items SET ");
    let mut wrote = false;

    apply_done_update(&mut qb, &mut wrote, payload.done);
    apply_category_update(&mut qb, &mut wrote, payload.category.clone())?;

    // `text` takes priority over structured fields.
    let did_text = apply_text_update(&mut qb, &mut wrote, &state, &payload).await?;
    if !did_text {
        let _did_struct =
            apply_structured_update(&mut qb, &mut wrote, &state, id, &payload).await?;
    }

    if !wrote {
        let dto = fetch_view_by_id(&state, id).await.map_err(internal_err)?;
        return Ok(Json(dto));
    }

    qb.push(" WHERE id = ");
    qb.push_bind(id);
    qb.push(" RETURNING id");

    let (rid,): (i64,) = qb
        .build_query_as()
        .fetch_one(&state.pool)
        .await
        .map_err(patch_update_err)?;

    let dto = fetch_view_by_id(&state, rid).await.map_err(internal_err)?;
    Ok(Json(dto))
}

/// DELETE /shopping/{id}
///
/// This is still a hard delete for explicit user intent.
/// The normal "tick off" flow should use PATCH { done: true }.
///
/// # Errors
/// Err if deleting the shopping item fails.
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let affected = sqlx::query("DELETE FROM shopping_items WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?
        .rows_affected();

    Ok(Json(serde_json::json!({ "deleted": affected })))
}

/// POST /shopping/merge
///
/// # Errors
/// Err if merging items (insert/update) fails.
/// Err if fetching the updated shopping list fails.
pub async fn merge_items(
    State(state): State<AppState>,
    Json(req): Json<MergeReq>,
) -> AppResult<Json<Vec<ShoppingItemView>>> {
    // BATCH NORMALIZATION: Collect all ingredient names first
    let raw_names: Vec<String> = req.items.iter().map(|it| it.name.clone()).collect();
    let normalized_names = normalize_ingredient_names_batch(&state, &raw_names).await;

    for (idx, it) in req.items.iter().enumerate() {
        // Parse embedded qty/unit from name, if any
        let parsed_name = parse_item_line(&it.name).unwrap_or_else(|| ParsedItem {
            qty: None,
            unit: None,
            name_raw: it.name.clone(),
            name_norm: normalize_name(&it.name),
        });

        // Prefer explicit fields; fall back to parsed-from-name
        let qty = it.quantity.or(parsed_name.qty);
        let unit = it.unit.as_ref().map(|u| u.trim().to_string()).or(parsed_name.unit);

        // Canonicalize units & quantities
        let (mut unit_norm, qty_norm) = to_canonical_qty_unit(unit.as_deref(), qty);

        // If there's no real quantity, treat as unitless so it merges with plain items
        if qty_norm.is_none() {
            unit_norm = None;
        }

        // Use batch-normalized name
        let name_normalized = &normalized_names[idx];
        let key = make_key(name_normalized, unit_norm);

        // Normalize/validate incoming category, if present
        let chosen_cat = it.category.as_ref().and_then(|s| {
            let s = crate::units::norm_whitespace(s);
            if s.is_empty() { None } else { Some(s) }
        });

        let chosen_cat = if let Some(c) = chosen_cat {
            if Category::from_str(&c).is_none() {
                return Err((StatusCode::BAD_REQUEST, "invalid category".into()).into());
            }
            Some(c)
        } else {
            // Reuse existing category if present to avoid redundant LLM calls.
            let existing_cat: Option<String> =
                sqlx::query_scalar(r"SELECT category FROM shopping_items WHERE key = ?")
                    .bind(&key)
                    .fetch_optional(&state.pool)
                    .await?;

            Some(match existing_cat {
                Some(c) if !c.trim().is_empty() => c,
                _ => guess_category(&state, &parsed_name.name_raw).await,
            })
        };

        // Prepare recipe_ids JSON array
        let recipe_ids_json = req.recipe_id.map_or_else(|| "[]".to_string(), |rid| format!("[{rid}]"));

        sqlx::query(
            r"
            INSERT INTO shopping_items (name, unit, quantity, done, key, category, recipe_ids)
            VALUES (?, ?, ?, 0, ?, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
              quantity = CASE
                WHEN excluded.quantity IS NULL THEN shopping_items.quantity
                WHEN shopping_items.quantity IS NULL THEN excluded.quantity
                ELSE shopping_items.quantity + excluded.quantity
              END,
              name = excluded.name,
              unit = excluded.unit,
              category = COALESCE(shopping_items.category, excluded.category),
              recipe_ids = (
                SELECT json_group_array(DISTINCT value)
                FROM (
                  SELECT value FROM json_each(shopping_items.recipe_ids)
                  UNION
                  SELECT value FROM json_each(excluded.recipe_ids)
                )
                WHERE value IS NOT NULL
              ),
              done = 0
            ",
        )
        .bind(name_normalized)
        .bind(unit_norm)
        .bind(qty_norm)
        .bind(&key)
        .bind(chosen_cat)
        .bind(&recipe_ids_json)
        .execute(&state.pool)
        .await?;
    }

    // Return the active (not done) list
    list(State(state)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_qty_token() {
        assert_eq!(parse_qty_token("2"), Some(2.0));
        assert_eq!(parse_qty_token("10"), Some(10.0));
        assert_eq!(parse_qty_token("1.5"), Some(1.5));
        assert_eq!(parse_qty_token("1,5"), Some(1.5));
        
        assert_eq!(parse_qty_token("2-3"), Some(2.5));
        assert_eq!(parse_qty_token("2–3"), Some(2.5));
        assert_eq!(parse_qty_token("1.5-2.5"), Some(2.0));
        assert_eq!(parse_qty_token("10–20"), Some(15.0));
        
        assert_eq!(parse_qty_token(""), None);
        assert_eq!(parse_qty_token("  "), None);
        assert_eq!(parse_qty_token("abc"), None);
    }

    #[test]
    fn test_normalize_unit_token() {
        assert_eq!(normalize_unit_token("g"), Some("g".to_string()));
        assert_eq!(normalize_unit_token("kg"), Some("kg".to_string()));
        assert_eq!(normalize_unit_token("ml"), Some("ml".to_string()));
        assert_eq!(normalize_unit_token("L"), Some("L".to_string()));
        assert_eq!(normalize_unit_token("tsp"), Some("tsp".to_string()));
        assert_eq!(normalize_unit_token("tbsp"), Some("tbsp".to_string()));
        
        assert_eq!(normalize_unit_token("gram"), Some("g".to_string()));
        assert_eq!(normalize_unit_token("GRAMS"), Some("g".to_string()));
        
        assert_eq!(normalize_unit_token(""), None);
        assert_eq!(normalize_unit_token("  "), None);
        assert_eq!(normalize_unit_token("cup"), None);
        assert_eq!(normalize_unit_token("oz"), None);
    }

    #[test]
    fn test_parse_item_line_simple() {
        let p = parse_item_line("milk").unwrap();
        assert_eq!(p.qty, None);
        assert_eq!(p.unit, None);
        assert_eq!(p.name_raw, "milk");
        assert_eq!(p.name_norm, "milk");
    }

    #[test]
    fn test_parse_item_line_with_qty() {
        let p = parse_item_line("2 apples").unwrap();
        assert_eq!(p.qty, Some(2.0));
        assert_eq!(p.unit, None);
        assert_eq!(p.name_raw, "apples");
        assert_eq!(p.name_norm, "apples");
    }

    #[test]
    fn test_parse_item_line_with_qty_and_unit() {
        let p = parse_item_line("120 g flour").unwrap();
        assert_eq!(p.qty, Some(120.0));
        assert_eq!(p.unit, Some("g".to_string()));
        assert_eq!(p.name_raw, "flour");
        assert_eq!(p.name_norm, "flour");
    }

    #[test]
    fn test_parse_item_line_range() {
        let p = parse_item_line("2-3 kg potatoes").unwrap();
        assert_eq!(p.qty, Some(2.5));
        assert_eq!(p.unit, Some("kg".to_string()));
        assert_eq!(p.name_raw, "potatoes");
        assert_eq!(p.name_norm, "potatoes");
    }

    #[test]
    fn test_parse_item_line_with_of() {
        let p = parse_item_line("2 kg of rice").unwrap();
        assert_eq!(p.qty, Some(2.0));
        assert_eq!(p.unit, Some("kg".to_string()));
        assert_eq!(p.name_raw, "rice");
        assert_eq!(p.name_norm, "rice");
    }

    #[test]
    fn test_parse_item_line_decimal() {
        let p = parse_item_line("1.5 L water").unwrap();
        assert_eq!(p.qty, Some(1.5));
        assert_eq!(p.unit, Some("L".to_string()));
        assert_eq!(p.name_raw, "water");
        assert_eq!(p.name_norm, "water");
    }

    #[test]
    fn test_parse_item_line_comma_decimal() {
        let p = parse_item_line("1,5 kg sugar").unwrap();
        assert_eq!(p.qty, Some(1.5));
        assert_eq!(p.unit, Some("kg".to_string()));
        assert_eq!(p.name_raw, "sugar");
        assert_eq!(p.name_norm, "sugar");
    }

    #[test]
    fn test_parse_item_line_case_insensitive() {
        let p = parse_item_line("200 ML Milk").unwrap();
        assert_eq!(p.qty, Some(200.0));
        assert_eq!(p.unit, Some("ml".to_string()));
        assert_eq!(p.name_raw, "Milk");
        assert_eq!(p.name_norm, "milk");
    }

    #[test]
    fn test_parse_item_line_missing_name_fallback() {
        let p = parse_item_line("2 kg").unwrap();
        assert_eq!(p.qty, None);
        assert_eq!(p.unit, None);
        assert_eq!(p.name_raw, "2 kg");
        assert_eq!(p.name_norm, "2 kg");
    }

    #[test]
    fn test_parse_item_line_empty() {
        assert!(parse_item_line("").is_none());
        assert!(parse_item_line("   ").is_none());
    }

    #[test]
    fn test_parse_item_line_unknown_unit() {
        let p = parse_item_line("2 cups flour").unwrap();
        assert_eq!(p.qty, Some(2.0));
        assert_eq!(p.unit, None);
        assert_eq!(p.name_raw, "cups flour");
        assert_eq!(p.name_norm, "cups flour");
    }

    #[test]
    fn test_parse_item_line_whitespace_normalization() {
        let p = parse_item_line("  2   kg    of   flour  ").unwrap();
        assert_eq!(p.qty, Some(2.0));
        assert_eq!(p.unit, Some("kg".to_string()));
        assert_eq!(p.name_raw, "flour");
        assert_eq!(p.name_norm, "flour");
    }
}
