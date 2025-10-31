use axum::http::StatusCode;
use axum::{
    Json,
    extract::{Path, State},
};
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use sqlx::Row;
use sqlx::sqlite::{SqliteArguments, SqliteRow};
use sqlx::{Arguments, QueryBuilder, Sqlite};

use crate::units::{normalize_name, to_canonical_qty_unit};
use crate::{error::AppResult, models::AppState};

// View model returned by endpoints in this file
#[derive(serde::Serialize, sqlx::FromRow, Clone)]
pub struct ShoppingItemView {
    pub id: i64,
    pub text: String,
    pub done: i64, // 0/1
    pub category: Option<String>,
}

/* ---------- Request types for merge ---------- */
#[derive(Serialize)]
pub struct ShoppingItemDto {
    pub id: i64,
    pub text: String,
    pub done: i64,
    pub category: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct UpdateShoppingItem {
    pub done: Option<bool>,
    pub category: Option<String>,
    pub text: Option<String>,
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
}

#[derive(Deserialize, Debug, Default)]
pub struct UpdateItem {
    #[serde(default)]
    pub done: Option<bool>,
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug)]
struct ParsedLine {
    quantity: Option<f64>,
    unit: Option<String>,
    name: String,
}

/* ---------- Helpers ---------- */

fn parse_line(raw: &str) -> ParsedLine {
    let s = raw.trim();
    // Normalize decimal comma to dot for SQLite REAL
    let s_norm = s.replace(',', ".");

    // Regex: optional qty, optional unit, then name
    // ^\s*(\d+(?:\.\d+)?)?\s*([A-Za-zµμ]+)?\s*(.*\S)?$
    // Make unit fairly permissive (letters + micro symbol)
    static RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(r#"^\s*(\d+(?:\.\d+)?)?\s*([A-Za-zµμ]+)?\s*(.*\S)?$"#).unwrap()
    });

    if let Some(c) = RE.captures(&s_norm) {
        let qty = c.get(1).and_then(|m| m.as_str().parse::<f64>().ok());
        let unit = c.get(2).map(|m| m.as_str().to_string());
        // If we had qty and unit but no name, or we had none of the above,
        // just fall back to the whole string as name.
        let name = c
            .get(3)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();

        // If name is empty, treat entire input as name (no structured fields)
        if name.is_empty() {
            return ParsedLine {
                quantity: None,
                unit: None,
                name: s.to_string(),
            };
        }

        ParsedLine {
            quantity: qty,
            unit,
            name,
        }
    } else {
        ParsedLine {
            quantity: None,
            unit: None,
            name: s.to_string(),
        }
    }
}

pub async fn patch_shopping_item(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateShoppingItem>,
) -> Result<Json<ShoppingItemDto>, (StatusCode, String)> {
    // Build dynamic UPDATE based on provided fields
    let mut qb = QueryBuilder::<Sqlite>::new("UPDATE shopping_items SET ");
    let mut wrote = false;

    // done
    if let Some(d) = payload.done {
        if wrote {
            qb.push(", ");
        }
        qb.push("done = ");
        qb.push_bind(if d { 1_i64 } else { 0_i64 });
        wrote = true;
    }

    // category (empty -> NULL)
    if let Some(mut cat) = payload.category.clone() {
        if wrote {
            qb.push(", ");
        }
        cat = cat.trim().to_string();
        if cat.is_empty() {
            qb.push("category = NULL");
        } else {
            qb.push("category = ");
            qb.push_bind(cat);
        }
        wrote = true;
    }

    // text -> parse to (quantity, unit, name)
    if let Some(ref t) = payload.text {
        let parsed = parse_line(t);

        if wrote {
            qb.push(", ");
        }
        qb.push("name = ");
        qb.push_bind(parsed.name);

        qb.push(", quantity = ");
        if let Some(q) = parsed.quantity {
            qb.push_bind(q);
        } else {
            qb.push("NULL");
        }

        qb.push(", unit = ");
        if let Some(u) = parsed.unit {
            // optional normalization
            let u_norm = match u.as_str() {
                s if s.eq_ignore_ascii_case("l") => "L".to_string(),
                s if s.eq_ignore_ascii_case("g") => "g".to_string(),
                s if s.eq_ignore_ascii_case("ml") => "ml".to_string(),
                _ => u,
            };
            qb.push_bind(u_norm);
        } else {
            qb.push("NULL");
        }

        wrote = true;
    }

    if !wrote {
        // Nothing to update — return current row
        let row = sqlx::query(
            r#"
            SELECT id,
                   CASE
                     WHEN quantity IS NOT NULL AND unit IS NOT NULL AND unit <> ''
                       THEN TRIM(printf('%g', quantity)) || ' ' || unit || ' ' || name
                     WHEN quantity IS NOT NULL
                       THEN TRIM(printf('%g', quantity)) || ' ' || name
                     ELSE name
                   END AS text,
                   done,
                   category
              FROM shopping_items
             WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_one(&state.pool) // <- FIX: use pool
        .await
        .map_err(internal_err)?;

        return Ok(Json(row_to_dto(row)));
    }

    qb.push(" WHERE id = ");
    qb.push_bind(id);
    qb.push(
        r#"
        RETURNING id,
           CASE
             WHEN quantity IS NOT NULL AND unit IS NOT NULL AND unit <> ''
               THEN TRIM(printf('%g', quantity)) || ' ' || unit || ' ' || name
             WHEN quantity IS NOT NULL
               THEN TRIM(printf('%g', quantity)) || ' ' || name
             ELSE name
           END AS text,
           done,
           category
        "#,
    );

    let row: SqliteRow = qb
        .build()
        .fetch_one(&state.pool) // <- FIX: use pool
        .await
        .map_err(internal_err)?;

    Ok(Json(row_to_dto(row)))
}

fn row_to_dto(row: SqliteRow) -> ShoppingItemDto {
    ShoppingItemDto {
        id: row.get::<i64, _>("id"),
        text: row.get::<String, _>("text"),
        done: row.get::<i64, _>("done"),
        category: row
            .try_get::<String, _>("category")
            .ok()
            .filter(|s| !s.is_empty()),
    }
}

fn internal_err<E: std::error::Error>(err: E) -> (axum::http::StatusCode, String) {
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        err.to_string(),
    )
}

fn guess_category(name_norm: &str) -> Option<&'static str> {
    let n = name_norm;
    // order matters; first match wins
    const MAP: &[(&[&str], &str)] = &[
        // Produce
        (
            &[
                "apple", "banana", "tomato", "cucumber", "lettuce", "carrot", "onion", "garlic",
                "pepper", "spinach", "potato", "avocado", "lemon", "lime", "orange", "berry",
            ],
            "Produce",
        ),
        // Dairy & Eggs
        (
            &[
                "milk",
                "yogurt",
                "cheese",
                "feta",
                "mozzarella",
                "butter",
                "cream",
                "egg",
            ],
            "Dairy",
        ),
        // Bakery
        (
            &["bread", "bun", "baguette", "roll", "tortilla", "pita"],
            "Bakery",
        ),
        // Meat & Fish
        (
            &[
                "chicken", "beef", "pork", "turkey", "ham", "salmon", "tuna", "shrimp", "sausage",
                "bacon",
            ],
            "Meat & Fish",
        ),
        // Pantry (dry goods, cans)
        (
            &[
                "flour",
                "sugar",
                "salt",
                "rice",
                "pasta",
                "noodle",
                "bean",
                "lentil",
                "canned",
                "tomato paste",
                "tomato sauce",
                "oil",
                "vinegar",
                "mustard",
                "ketchup",
                "honey",
            ],
            "Pantry",
        ),
        // Spices
        (
            &[
                "cumin",
                "paprika",
                "oregano",
                "basil",
                "thyme",
                "coriander",
                "curry",
                "chili",
                "turmeric",
                "peppercorn",
                "spice",
            ],
            "Spices",
        ),
        // Frozen
        (
            &["frozen", "ice cream", "frozen berries", "frozen peas"],
            "Frozen",
        ),
        // Beverages
        (
            &["coffee", "tea", "juice", "soda", "water", "sparkling"],
            "Beverages",
        ),
        // Household / Other
        (
            &[
                "paper",
                "towel",
                "foil",
                "wrap",
                "detergent",
                "soap",
                "shampoo",
                "bag",
                "trash",
            ],
            "Household",
        ),
    ];
    for (needles, cat) in MAP {
        if needles.iter().any(|k| n.contains(k)) {
            return Some(*cat);
        }
    }
    None
}

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
fn parse_free_text_item(s: &str) -> Option<(String, Option<&str>, Option<f64>)> {
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
    let (unit_norm, qty_norm) = to_canonical_qty_unit(unit.as_deref(), qty);

    Some((name_norm, unit_norm, qty_norm))
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
pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<ShoppingItemView>>> {
    let rows = sqlx::query_as::<_, ShoppingItemView>(
        r#"
        SELECT id,
               CASE
                 WHEN quantity IS NOT NULL AND unit IS NOT NULL AND unit <> ''
                   THEN TRIM(printf('%g', quantity)) || ' ' || unit || ' ' || name
                 WHEN quantity IS NOT NULL
                   THEN TRIM(printf('%g', quantity)) || ' ' || name
                 ELSE name
               END AS text,
               done,
               category
          FROM shopping_items
         ORDER BY id
        "#,
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

// POST /shopping { "text": "..." }
#[derive(Deserialize)]
pub struct NewItem {
    pub text: String,
}

pub async fn create(
    State(state): State<AppState>,
    Json(new): Json<NewItem>,
) -> AppResult<Json<ShoppingItemView>> {
    let text = new.text.trim();
    if text.is_empty() {
        return Err(anyhow::anyhow!("empty shopping item").into());
    }

    // If the line looks structured, upsert by (unit,name) and sum quantity.
    if let Some((name_norm, unit_norm, qty_norm)) = parse_free_text_item(text) {
        let key = make_key(&name_norm, unit_norm);
        let category_guess = guess_category(&name_norm).map(|s| s.to_string());

        sqlx::query(
            r#"
    INSERT INTO shopping_items (name, unit, quantity, done, key, category)
    VALUES (?, ?, ?, 0, ?, ?)
    ON CONFLICT(key) DO UPDATE SET
      quantity = COALESCE(shopping_items.quantity, 0)
               + COALESCE(excluded.quantity, 0),
      -- keep existing category if present; otherwise take the new one
      category = COALESCE(shopping_items.category, excluded.category)
    "#,
        )
        .bind(&name_norm)
        .bind(unit_norm)
        .bind(qty_norm)
        .bind(&key)
        .bind(&category_guess)
        .execute(&state.pool)
        .await?;

        let row = sqlx::query_as::<_, ShoppingItemView>(
            r#"
            SELECT id,
                   CASE
                     WHEN quantity IS NOT NULL AND unit IS NOT NULL AND unit <> ''
                       THEN TRIM(printf('%g', quantity)) || ' ' || unit || ' ' || name
                     WHEN quantity IS NOT NULL
                       THEN TRIM(printf('%g', quantity)) || ' ' || name
                     ELSE name
                   END AS text,
                   done,
                   category
              FROM shopping_items
             WHERE key = ?
            "#,
        )
        .bind(&key)
        .fetch_one(&state.pool)
        .await?;

        return Ok(Json(row));
    }

    // Fallback: plain-text item (no qty/unit)
    let name_norm = normalize_name(text);
    let category_guess = guess_category(&name_norm).map(|s| s.to_string());
    let key = make_key(&name_norm, None);

    sqlx::query(
        r#"
    INSERT INTO shopping_items (name, unit, quantity, done, key, category)
    VALUES (?, NULL, NULL, 0, ?, ?)
    ON CONFLICT(key) DO NOTHING
    "#,
    )
    .bind(&name_norm)
    .bind(&key)
    .bind(&category_guess)
    .execute(&state.pool)
    .await?;

    let row = sqlx::query_as::<_, ShoppingItemView>(
        r#"
        SELECT id,
               CASE
                 WHEN quantity IS NOT NULL AND unit IS NOT NULL AND unit <> ''
                   THEN TRIM(printf('%g', quantity)) || ' ' || unit || ' ' || name
                 WHEN quantity IS NOT NULL
                   THEN TRIM(printf('%g', quantity)) || ' ' || name
                 ELSE name
               END AS text,
               done,
               category
          FROM shopping_items
         WHERE key = ?
        "#,
    )
    .bind(&key)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(row))
}

// PATCH /shopping/:id  body can be: {"done":true}, {"category":"Dairy"}, or both
pub async fn toggle_done(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(t): Json<UpdateItem>,
) -> AppResult<Json<ShoppingItemView>> {
    // Build dynamic SET clause
    let mut sets: Vec<&'static str> = Vec::new();
    let mut args = SqliteArguments::default();

    if let Some(done) = t.done {
        sets.push("done = ?");
        args.add(if done { 1i64 } else { 0i64 })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    if let Some(cat_raw) = t.category {
        // Normalize; empty -> NULL
        let cat_norm = crate::units::norm_whitespace(&cat_raw);
        if cat_norm.is_empty() {
            sets.push("category = NULL");
        } else {
            sets.push("category = ?");
            args.add(cat_norm)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
    }

    if sets.is_empty() {
        // Nothing to update
        return Err(StatusCode::BAD_REQUEST.into());
    }

    // WHERE id = ?
    sets.push("-- sentinel");
    let sql = format!(
        r#"
        UPDATE shopping_items
           SET {sets}
         WHERE id = ?
         RETURNING id,
           CASE
             WHEN quantity IS NOT NULL AND unit IS NOT NULL AND unit <> ''
               THEN TRIM(printf('%g', quantity)) || ' ' || unit || ' ' || name
             WHEN quantity IS NOT NULL
               THEN TRIM(printf('%g', quantity)) || ' ' || name
             ELSE name
           END AS text,
           done,
           category
        "#,
        sets = sets[..sets.len() - 1].join(", ")
    );

    args.add(id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let row = sqlx::query_as_with::<_, ShoppingItemView, _>(&sql, args)
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
) -> AppResult<Json<Vec<ShoppingItemView>>> {
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
        let (mut unit_norm, qty_norm) = to_canonical_qty_unit(unit.as_deref(), qty);

        // 4) if there's no real quantity, treat as *unitless* so it merges with plain items
        if qty_norm.is_none() {
            unit_norm = None;
        }

        // 5) key and upsert (avoid turning NULL into 0)
        let key = make_key(&base_name, unit_norm);

        let chosen_cat = it
            .category
            .and_then(|s| {
                let s = crate::units::norm_whitespace(&s);
                if s.is_empty() { None } else { Some(s) }
            })
            .or_else(|| guess_category(&base_name).map(|s| s.to_string()));

        sqlx::query(
            r#"
    INSERT INTO shopping_items (name, unit, quantity, done, key, category)
    VALUES (?, ?, ?, 0, ?, ?)
    ON CONFLICT(key) DO UPDATE SET
      quantity = CASE
        WHEN excluded.quantity IS NULL THEN shopping_items.quantity
        WHEN shopping_items.quantity IS NULL THEN excluded.quantity
        ELSE shopping_items.quantity + excluded.quantity
      END,
      -- keep normalized name and canonical unit
      name = excluded.name,
      unit = excluded.unit,
      category = COALESCE(shopping_items.category, excluded.category)
    "#,
        )
        .bind(&base_name)
        .bind(unit_norm) // Option<&str>
        .bind(qty_norm) // Option<f64>
        .bind(&key)
        .bind(chosen_cat) // Option<String>
        .execute(&state.pool)
        .await?;
    }

    // Return list in the usual "view" shape
    let rows: Vec<ShoppingItemView> = sqlx::query_as::<_, ShoppingItemView>(
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
          done,
          category
        FROM shopping_items
        ORDER BY id
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows))
}
