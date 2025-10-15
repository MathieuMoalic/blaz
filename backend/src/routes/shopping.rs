use axum::{
    Json,
    extract::{Path, State},
};

use serde::Deserialize;

use crate::{
    error::AppResult,
    models::{AppState, NewItem, ShoppingItem, ToggleItem},
};

/* ---------- Request types for merge ---------- */

#[derive(Deserialize, Clone)]
pub struct InIngredient {
    pub quantity: Option<f64>,
    pub unit: Option<String>, // "g","kg","ml","L","tsp","tbsp" or null
    pub name: String,
}

#[derive(Deserialize)]
pub struct MergeReq {
    pub items: Vec<InIngredient>,
}

/* ---------- Helpers ---------- */

/// If `name` starts with "<qty> [unit] <rest>", pull qty+unit out and return them
/// along with the cleaned name. Unit is optional here.
fn strip_leading_qty_unit(name: &str) -> (Option<f64>, Option<String>, String) {
    let s = name.trim().to_lowercase().replace(',', ".");
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.is_empty() {
        return (None, None, name.trim().to_string());
    }

    // parse number or range in the first token
    let mut qty: Option<f64> = None;
    if let Some(p0) = parts.first() {
        if let Some((a, b)) = p0.split_once('-').or_else(|| p0.split_once('–')) {
            if let (Ok(x), Ok(y)) = (a.parse::<f64>(), b.parse::<f64>()) {
                qty = Some((x + y) / 2.0);
            }
        } else if let Ok(x) = p0.parse::<f64>() {
            qty = Some(x);
        }
    }
    if qty.is_none() {
        return (None, None, name.trim().to_string());
    }

    // optional unit in the second token
    let mut unit: Option<String> = None;
    let mut i = 1;
    if let Some(p1) = parts.get(1) {
        let u = p1.trim().trim_end_matches('s').to_lowercase();
        unit = match u.as_str() {
            "g" | "gram" => Some("g".into()),
            "kg" | "kilogram" => Some("kg".into()),
            "ml" | "milliliter" | "millilitre" => Some("ml".into()),
            "l" | "liter" | "litre" => Some("L".into()),
            "tsp" | "teaspoon" => Some("tsp".into()),
            "tbsp" | "tablespoon" => Some("tbsp".into()),
            _ => None,
        };
        if unit.is_some() {
            i = 2;
        }
    }

    // optional "of"
    if parts.get(i).copied() == Some("of") {
        i += 1;
    }
    if i >= parts.len() {
        return (qty, unit, String::new());
    }
    let clean = parts[i..].join(" ");
    (qty, unit, clean)
}

/// Try to parse a free-text line like:
/// "100 ml water", "1 banana", "0.5 bananas", "2-3 apples", "1 cup of sugar"
/// Returns (normalized_name, canonical_unit (or None), quantity).
fn parse_free_text_item(s: &str) -> Option<(String, Option<String>, Option<f64>)> {
    let raw = s.trim();
    if raw.is_empty() {
        return None;
    }

    // tokens; normalize commas to dots for decimals
    let tokens: Vec<String> = raw
        .split_whitespace()
        .map(|t| t.trim().replace(',', "."))
        .collect();
    if tokens.is_empty() {
        return None;
    }

    // 1) parse number (supports "a-b" or "a–b" ranges)
    let mut qty: Option<f64> = None;
    let t0 = tokens.first().map(|s| s.as_str()).unwrap_or("");
    let mut name_start_idx = 1;

    if let Some((a, b)) = t0.split_once('-').or_else(|| t0.split_once('–')) {
        if let (Ok(x), Ok(y)) = (a.parse::<f64>(), b.parse::<f64>()) {
            qty = Some((x + y) / 2.0);
        }
    } else if let Ok(x) = t0.parse::<f64>() {
        qty = Some(x);
    } else {
        // doesn’t start with a number -> not a structured line
        return None;
    }

    // 2) parse unit (optional)
    let mut unit: Option<String> = None;
    if let Some(t1) = tokens.get(1) {
        let u = t1.trim().trim_end_matches('s').to_lowercase();
        unit = match u.as_str() {
            "g" | "gram" => Some("g".into()),
            "kg" | "kilogram" => Some("kg".into()),
            "ml" | "milliliter" | "millilitre" => Some("ml".into()),
            "l" | "liter" | "litre" => Some("L".into()),
            "tsp" | "teaspoon" => Some("tsp".into()),
            "tbsp" | "tablespoon" => Some("tbsp".into()),
            _ => None,
        };
        if unit.is_some() {
            name_start_idx = 2;
        }
    }

    // optional "of"
    if tokens.get(name_start_idx).map(|s| s.as_str()) == Some("of") {
        name_start_idx += 1;
    }

    if name_start_idx >= tokens.len() {
        return None;
    }

    // 3) normalize name
    let name_raw = tokens[name_start_idx..].join(" ");
    let name_norm = normalize_name(&name_raw);

    // 4) canonicalize quantity+unit (kg->g, L->ml, tbsp/tsp->ml etc.)
    let (unit_norm, qty_norm) = to_canonical_unit(unit.as_deref(), qty);

    Some((name_norm, unit_norm, qty_norm))
}

fn normalize_name(s: &str) -> String {
    // trim, lowercase, collapse internal whitespace
    let mut out = String::with_capacity(s.len());
    let mut ws = false;
    for ch in s.trim().to_lowercase().chars() {
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

/// Convert to canonical base units used for merging.
/// - kg -> g
/// - L  -> ml
/// - tbsp -> ml (15)
/// - tsp  -> ml (5)
fn to_canonical_unit(unit: Option<&str>, qty: Option<f64>) -> (Option<String>, Option<f64>) {
    let u = unit
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    match (u.as_deref(), qty) {
        (Some("kg"), Some(q)) => (Some("g".into()), Some(q * 1000.0)),
        (Some("l"), Some(q)) => (Some("ml".into()), Some(q * 1000.0)),
        (Some("tbsp"), Some(q)) => (Some("ml".into()), Some(q * 15.0)),
        (Some("tsp"), Some(q)) => (Some("ml".into()), Some(q * 5.0)),
        (u, q) => (u.map(|s| s.to_string()), q),
    }
}

/// Unique key used for merging rows: "<unit>|<name>" with normalized name/unit.
/// For unit-less items the key starts with a leading pipe: "|<name>".
fn make_key(name_norm: &str, unit_norm: Option<&str>) -> String {
    match unit_norm {
        Some(u) if !u.is_empty() => format!("{u}|{name_norm}"),
        _ => format!("|{name_norm}"),
    }
}

/* ---------- Routes ---------- */

// GET /shopping
pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<ShoppingItem>>> {
    // Format a display text from (quantity,unit,name) so the frontend can keep using ShoppingItem
    let rows = sqlx::query_as::<_, ShoppingItem>(
        r#"
        SELECT id,
               CASE
                 WHEN quantity IS NOT NULL AND unit IS NOT NULL AND unit <> ''
                   THEN TRIM(printf('%g', quantity)) || ' ' || unit || ' ' || name
                 WHEN quantity IS NOT NULL
                   THEN TRIM(printf('%g', quantity)) || ' ' || name
                 ELSE name
               END AS text,
               done
          FROM shopping_items
         ORDER BY id
        "#,
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

// POST /shopping { text }
// POST /shopping { text }
pub async fn create(
    State(state): State<AppState>,
    Json(new): Json<NewItem>,
) -> AppResult<Json<ShoppingItem>> {
    let text = new.text.trim();
    if text.is_empty() {
        return Err(anyhow::anyhow!("empty shopping item").into());
    }

    // If the line looks structured, upsert by (unit,name) and sum quantity.
    if let Some((name_norm, unit_norm, qty_norm)) = parse_free_text_item(text) {
        let key = make_key(&name_norm, unit_norm.as_deref());

        sqlx::query(
            r#"
            INSERT INTO shopping_items (name, unit, quantity, done, key)
            VALUES (?, ?, ?, 0, ?)
            ON CONFLICT(key) DO UPDATE SET
              quantity = COALESCE(shopping_items.quantity, 0)
                       + COALESCE(excluded.quantity, 0)
            "#,
        )
        .bind(&name_norm)
        .bind(&unit_norm)
        .bind(qty_norm)
        .bind(&key)
        .execute(&state.pool)
        .await?;

        let row = sqlx::query_as::<_, ShoppingItem>(
            r#"
            SELECT id,
                   CASE
                     WHEN quantity IS NOT NULL AND unit IS NOT NULL AND unit <> ''
                       THEN TRIM(printf('%g', quantity)) || ' ' || unit || ' ' || name
                     WHEN quantity IS NOT NULL
                       THEN TRIM(printf('%g', quantity)) || ' ' || name
                     ELSE name
                   END AS text,
                   done
              FROM shopping_items
             WHERE key = ?
            "#,
        )
        .bind(&key)
        .fetch_one(&state.pool)
        .await?;

        return Ok(Json(row));
    }

    // Fallback: plain-text item (no qty/unit) — original behavior
    let name_norm = normalize_name(text);
    let key = make_key(&name_norm, None);

    sqlx::query(
        r#"
        INSERT INTO shopping_items (name, unit, quantity, done, key)
        VALUES (?, NULL, NULL, 0, ?)
        ON CONFLICT(key) DO NOTHING
        "#,
    )
    .bind(&name_norm)
    .bind(&key)
    .execute(&state.pool)
    .await?;

    let row = sqlx::query_as::<_, ShoppingItem>(
        r#"
        SELECT id,
               CASE
                 WHEN quantity IS NOT NULL AND unit IS NOT NULL AND unit <> ''
                   THEN TRIM(printf('%g', quantity)) || ' ' || unit || ' ' || name
                 WHEN quantity IS NOT NULL
                   THEN TRIM(printf('%g', quantity)) || ' ' || name
                 ELSE name
               END AS text,
               done
          FROM shopping_items
         WHERE key = ?
        "#,
    )
    .bind(&key)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(row))
}

// PATCH /shopping/:id { done: bool }
pub async fn toggle_done(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(t): Json<ToggleItem>,
) -> AppResult<Json<ShoppingItem>> {
    let done_i64 = if t.done { 1 } else { 0 };

    let row = sqlx::query_as::<_, ShoppingItem>(
        r#"
        UPDATE shopping_items
           SET done = ?
         WHERE id = ?
         RETURNING id,
           CASE
             WHEN quantity IS NOT NULL AND unit IS NOT NULL AND unit <> ''
               THEN TRIM(printf('%g', quantity)) || ' ' || unit || ' ' || name
             WHEN quantity IS NOT NULL
               THEN TRIM(printf('%g', quantity)) || ' ' || name
             ELSE name
           END AS text,
           done
        "#,
    )
    .bind(done_i64)
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(row))
}

// DELETE /shopping/:id
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

// POST /shopping/merge  { items: InIngredient[] }
pub async fn merge_items(
    State(state): State<AppState>,
    Json(req): Json<MergeReq>,
) -> AppResult<Json<Vec<ShoppingItem>>> {
    for it in req.items {
        // 1) start from provided fields
        let mut qty = it.quantity;
        let mut unit = it.unit.map(|u| u.trim().to_lowercase());

        // 2) strip any leading "<qty> <unit>" from the *name* if present
        let (qty_from_name, unit_from_name, clean_name) = strip_leading_qty_unit(&it.name);
        let base_name = normalize_name(&clean_name);

        // prefer explicit fields; fall back to parsed-from-name
        if qty.is_none() {
            qty = qty_from_name;
        }
        if unit.is_none() {
            unit = unit_from_name;
        }

        // 3) canonicalize units & quantities
        let (mut unit_norm, qty_norm) = to_canonical_unit(unit.as_deref(), qty);

        // 4) if there's no real quantity, treat as *unitless* so it merges with plain items
        if qty_norm.is_none() {
            unit_norm = None;
        }

        // 5) key and upsert (avoid turning NULL into 0)
        let key = make_key(&base_name, unit_norm.as_deref());

        sqlx::query(
            r#"
            INSERT INTO shopping_items (name, unit, quantity, done, key)
            VALUES (?, ?, ?, 0, ?)
            ON CONFLICT(key) DO UPDATE SET
              quantity = CASE
                WHEN excluded.quantity IS NULL THEN shopping_items.quantity
                WHEN shopping_items.quantity IS NULL THEN excluded.quantity
                ELSE shopping_items.quantity + excluded.quantity
              END,
              -- keep normalized name and canonical unit
              name = excluded.name,
              unit = excluded.unit
            "#,
        )
        .bind(&base_name)
        .bind(&unit_norm)
        .bind(qty_norm)
        .bind(&key)
        .execute(&state.pool)
        .await?;
    }

    // Return list in the usual "view" shape
    let rows: Vec<ShoppingItem> = sqlx::query_as::<_, ShoppingItem>(
        r#"
        SELECT
          id,
          CASE
            WHEN quantity IS NOT NULL AND unit IS NOT NULL AND unit <> ''
              THEN TRIM(printf('%g', quantity)) || ' ' || unit || ' ' || name
            WHEN quantity IS NOT NULL
              THEN TRIM(printf('%g', quantity)) || ' ' || name
            ELSE name
          END AS text,
          done
        FROM shopping_items
        ORDER BY id
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows))
}
