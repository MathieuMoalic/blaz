use axum::http::StatusCode;
use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteArguments;
use sqlx::{Arguments, QueryBuilder, Sqlite};

use crate::error::AppResult;
use crate::models::{AppState, NewItem, ShoppingItemView};
use crate::units::{canon_unit_str, normalize_name, to_canonical_qty_unit};

/* ---------- Request/response types ---------- */

#[derive(Serialize, sqlx::FromRow)]
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

// PATCH helper body (kept local)
#[derive(Deserialize, Debug, Default)]
pub struct UpdateItem {
    #[serde(default)]
    pub done: Option<bool>,
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedItem {
    pub qty: Option<f64>,
    pub unit: Option<String>, // normalized short unit, e.g. "g","kg","ml","L","tsp","tbsp"
    pub name_raw: String,     // what we’d store in DB as name if parsing succeeded
    pub name_norm: String,    // normalized for merge key/category
}

#[derive(Debug, Clone, Copy)]
enum ParseMode {
    Create,
    Patch,
    Merge,
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
fn parse_item_line(raw: &str, _mode: ParseMode) -> Option<ParsedItem> {
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
        let name_raw = raw.to_string();
        let name_norm = normalize_name(&name_raw);
        return Some(ParsedItem {
            qty: None,
            unit: None,
            name_raw,
            name_norm,
        });
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
        // Mirror old parse_line fallback: ignore parsed qty/unit if name is missing
        let name_raw = raw.to_string();
        let name_norm = normalize_name(&name_raw);
        return Some(ParsedItem {
            qty: None,
            unit: None,
            name_raw,
            name_norm,
        });
    }

    let name_raw = tokens[idx..].join(" ");
    let name_norm = normalize_name(&name_raw);

    Some(ParsedItem {
        qty,
        unit,
        name_raw,
        name_norm,
    })
}

/* ---------- Other helpers ---------- */

fn internal_err<E: std::error::Error>(err: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

async fn fetch_view_by_id(state: &AppState, id: i64) -> Result<ShoppingItemView, sqlx::Error> {
    sqlx::query_as::<_, ShoppingItemView>(
        r"
        SELECT id, text, done, category
          FROM shopping_items_view
         WHERE id = ?
        ",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
}

async fn fetch_dto_by_id(state: &AppState, id: i64) -> Result<ShoppingItemDto, sqlx::Error> {
    sqlx::query_as::<_, ShoppingItemDto>(
        r"
        SELECT id, text, done, category
          FROM shopping_items_view
         WHERE id = ?
        ",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
}

fn guess_category(name_norm: &str) -> Option<&'static str> {
    const MAP: &[(&[&str], &str)] = &[
        (
            &[
                "apple", "banana", "tomato", "cucumber", "lettuce", "carrot", "onion", "garlic",
                "pepper", "spinach", "potato", "avocado", "lemon", "lime", "orange", "berry",
            ],
            "Produce",
        ),
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
        (
            &["bread", "bun", "baguette", "roll", "tortilla", "pita"],
            "Bakery",
        ),
        (
            &[
                "chicken", "beef", "pork", "turkey", "ham", "salmon", "tuna", "shrimp", "sausage",
                "bacon",
            ],
            "Meat & Fish",
        ),
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
        (
            &["frozen", "ice cream", "frozen berries", "frozen peas"],
            "Frozen",
        ),
        (
            &["coffee", "tea", "juice", "soda", "water", "sparkling"],
            "Beverages",
        ),
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
    let n = name_norm;
    for (needles, cat) in MAP {
        if needles.iter().any(|k| n.contains(k)) {
            return Some(*cat);
        }
    }
    None
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

/// GET /shopping
///
/// # Errors
/// Err if querying the database fails.
pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<ShoppingItemView>>> {
    let rows = sqlx::query_as::<_, ShoppingItemView>(
        r"
        SELECT id, text, done, category
          FROM shopping_items_view
         ORDER BY id
        ",
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows))
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
        return Err(anyhow::anyhow!("empty shopping item").into());
    }

    let parsed = parse_item_line(text, ParseMode::Create)
        .ok_or_else(|| anyhow::anyhow!("empty shopping item"))?;

    // Structured path only if a leading qty was detected
    if parsed.qty.is_some() {
        let (unit_norm, qty_norm) = to_canonical_qty_unit(parsed.unit.as_deref(), parsed.qty);
        let key = make_key(&parsed.name_norm, unit_norm);
        let category_guess =
            guess_category(&parsed.name_norm).map(std::string::ToString::to_string);

        sqlx::query(
            r"
            INSERT INTO shopping_items (name, unit, quantity, done, key, category)
            VALUES (?, ?, ?, 0, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
              quantity = COALESCE(shopping_items.quantity, 0)
                       + COALESCE(excluded.quantity, 0),
              category = COALESCE(shopping_items.category, excluded.category)
            ",
        )
        .bind(&parsed.name_norm)
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

    // Fallback: plain-text item
    let name_norm = parsed.name_norm;
    let category_guess = guess_category(&name_norm).map(std::string::ToString::to_string);
    let key = make_key(&name_norm, None);

    sqlx::query(
        r"
        INSERT INTO shopping_items (name, unit, quantity, done, key, category)
        VALUES (?, NULL, NULL, 0, ?, ?)
        ON CONFLICT(key) DO NOTHING
        ",
    )
    .bind(&name_norm)
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

/// PATCH /shopping/{id}
///
/// # Errors
/// Err with `400` if the provided text is empty.
/// Err with `500` if updating or fetching the item fails.
pub async fn patch_shopping_item(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateShoppingItem>,
) -> Result<Json<ShoppingItemDto>, (StatusCode, String)> {
    let mut qb = QueryBuilder::<Sqlite>::new("UPDATE shopping_items SET ");
    let mut wrote = false;

    if let Some(d) = payload.done {
        if wrote {
            qb.push(", ");
        }
        qb.push("done = ");
        qb.push_bind(i64::from(d));
        wrote = true;
    }

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

    if let Some(ref t) = payload.text {
        let parsed = parse_item_line(t, ParseMode::Patch)
            .ok_or_else(|| (StatusCode::BAD_REQUEST, "empty text".into()))?;

        if wrote {
            qb.push(", ");
        }

        qb.push("name = ");
        qb.push_bind(parsed.name_raw);

        qb.push(", quantity = ");
        if let Some(q) = parsed.qty {
            qb.push_bind(q);
        } else {
            qb.push("NULL");
        }

        qb.push(", unit = ");
        if let Some(u) = parsed.unit {
            qb.push_bind(u);
        } else {
            qb.push("NULL");
        }

        wrote = true;
    }

    if !wrote {
        let dto = fetch_dto_by_id(&state, id).await.map_err(internal_err)?;
        return Ok(Json(dto));
    }

    qb.push(" WHERE id = ");
    qb.push_bind(id);
    qb.push(" RETURNING id");

    let (rid,): (i64,) = qb
        .build_query_as()
        .fetch_one(&state.pool)
        .await
        .map_err(internal_err)?;

    let dto = fetch_dto_by_id(&state, rid).await.map_err(internal_err)?;
    Ok(Json(dto))
}

/// PATCH /shopping/{id}
///
/// # Errors
/// Err with `400` if no fields are provided to update.
/// Err if updating or fetching the shopping item fails.
pub async fn toggle_done(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(t): Json<UpdateItem>,
) -> AppResult<Json<ShoppingItemView>> {
    let mut sets: Vec<&'static str> = Vec::new();
    let mut args = SqliteArguments::default();

    if let Some(done) = t.done {
        sets.push("done = ?");
        args.add(i64::from(done))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    if let Some(cat_raw) = t.category {
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
        return Err(StatusCode::BAD_REQUEST.into());
    }

    let sql = format!(
        r"
        UPDATE shopping_items
           SET {}
         WHERE id = ?
         RETURNING id
        ",
        sets.join(", ")
    );

    args.add(id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (rid,): (i64,) = sqlx::query_as_with(&sql, args)
        .fetch_one(&state.pool)
        .await?;

    let row = fetch_view_by_id(&state, rid).await?;
    Ok(Json(row))
}

/// DELETE /shopping/{id}
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
    for it in req.items {
        // Parse embedded qty/unit from name, if any
        let parsed_name =
            parse_item_line(&it.name, ParseMode::Merge).unwrap_or_else(|| ParsedItem {
                qty: None,
                unit: None,
                name_raw: it.name.clone(),
                name_norm: normalize_name(&it.name),
            });

        // Prefer explicit fields; fall back to parsed-from-name
        let qty = it.quantity.or(parsed_name.qty);
        let unit = it.unit.map(|u| u.trim().to_string()).or(parsed_name.unit);

        // Canonicalize units & quantities
        let (mut unit_norm, qty_norm) = to_canonical_qty_unit(unit.as_deref(), qty);

        // If there's no real quantity, treat as unitless so it merges with plain items
        if qty_norm.is_none() {
            unit_norm = None;
        }

        let key = make_key(&parsed_name.name_norm, unit_norm);

        let chosen_cat = it
            .category
            .and_then(|s| {
                let s = crate::units::norm_whitespace(&s);
                if s.is_empty() { None } else { Some(s) }
            })
            .or_else(|| {
                guess_category(&parsed_name.name_norm).map(std::string::ToString::to_string)
            });

        sqlx::query(
            r"
            INSERT INTO shopping_items (name, unit, quantity, done, key, category)
            VALUES (?, ?, ?, 0, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
              quantity = CASE
                WHEN excluded.quantity IS NULL THEN shopping_items.quantity
                WHEN shopping_items.quantity IS NULL THEN excluded.quantity
                ELSE shopping_items.quantity + excluded.quantity
              END,
              name = excluded.name,
              unit = excluded.unit,
              category = COALESCE(shopping_items.category, excluded.category)
            ",
        )
        .bind(&parsed_name.name_norm) // keep normalized name in DB for consistency
        .bind(unit_norm)
        .bind(qty_norm)
        .bind(&key)
        .bind(chosen_cat)
        .execute(&state.pool)
        .await?;
    }

    let rows: Vec<ShoppingItemView> = sqlx::query_as::<_, ShoppingItemView>(
        r"
        SELECT id, text, done, category
          FROM shopping_items_view
         ORDER BY id
        ",
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows))
}
