use axum::http::StatusCode;
use axum::{
    Json,
    extract::{Path, State},
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite};

use crate::error::AppResult;
use crate::models::{AppState, NewItem, ShoppingItemView};
use crate::units::{normalize_name, to_canonical_qty_unit};

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

#[derive(Deserialize, Debug, Default)]
pub struct UpdateItem {
    #[serde(default)]
    pub done: Option<bool>,
    #[serde(default)]
    pub category: Option<String>,
}

/* ---------- Helpers ---------- */

#[derive(Debug)]
struct ParsedLine {
    quantity: Option<f64>,
    unit: Option<String>,
    name: String,
}

fn parse_line(raw: &str) -> ParsedLine {
    let s = raw.trim();
    let s_norm = s.replace(',', ".");

    static RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(r#"^\s*(\d+(?:\.\d+)?)?\s*([A-Za-zµμ]+)?\s*(.*\S)?$"#).unwrap()
    });

    if let Some(c) = RE.captures(&s_norm) {
        let qty = c.get(1).and_then(|m| m.as_str().parse::<f64>().ok());
        let unit = c.get(2).map(|m| m.as_str().to_string());
        let name = c
            .get(3)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();

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

fn internal_err<E: std::error::Error>(err: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

async fn fetch_view_by_id(state: &AppState, id: i64) -> Result<ShoppingItemView, sqlx::Error> {
    sqlx::query_as::<_, ShoppingItemView>(
        r#"
        SELECT id, text, done, category
          FROM shopping_items_view
         WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
}

async fn fetch_dto_by_id(state: &AppState, id: i64) -> Result<ShoppingItemDto, sqlx::Error> {
    sqlx::query_as::<_, ShoppingItemDto>(
        r#"
        SELECT id, text, done, category
          FROM shopping_items_view
         WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
}

fn guess_category(name_norm: &str) -> Option<&'static str> {
    let n = name_norm;
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
    for (needles, cat) in MAP {
        if needles.iter().any(|k| n.contains(k)) {
            return Some(*cat);
        }
    }
    None
}

fn strip_leading_qty_unit(name: &str) -> (Option<f64>, Option<String>, String) {
    let s = name.trim().to_lowercase().replace(',', ".");
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.is_empty() {
        return (None, None, name.trim().to_string());
    }

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

    if parts.get(i).copied() == Some("of") {
        i += 1;
    }
    if i >= parts.len() {
        return (qty, unit, String::new());
    }

    let clean = parts[i..].join(" ");
    (qty, unit, clean)
}

fn parse_free_text_item(s: &str) -> Option<(String, Option<&str>, Option<f64>)> {
    let raw = s.trim();
    if raw.is_empty() {
        return None;
    }

    let tokens: Vec<String> = raw
        .split_whitespace()
        .map(|t| t.trim().replace(',', "."))
        .collect();
    if tokens.is_empty() {
        return None;
    }

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
        return None;
    }

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

    if tokens.get(name_start_idx).map(|s| s.as_str()) == Some("of") {
        name_start_idx += 1;
    }
    if name_start_idx >= tokens.len() {
        return None;
    }

    let name_raw = tokens[name_start_idx..].join(" ");
    let name_norm = normalize_name(&name_raw);

    let (unit_norm, qty_norm) = to_canonical_qty_unit(unit.as_deref(), qty);

    Some((name_norm, unit_norm, qty_norm))
}

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
        SELECT id, text, done, category
          FROM shopping_items_view
         ORDER BY id
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows))
}

// POST /shopping { "text": "..." }
pub async fn create(
    State(state): State<AppState>,
    Json(new): Json<NewItem>,
) -> AppResult<Json<ShoppingItemView>> {
    let text = new.text.trim();
    if text.is_empty() {
        return Err(anyhow::anyhow!("empty shopping item").into());
    }

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

        let (id,): (i64,) = sqlx::query_as("SELECT id FROM shopping_items WHERE key = ?")
            .bind(&key)
            .fetch_one(&state.pool)
            .await?;

        let row = fetch_view_by_id(&state, id).await?;
        return Ok(Json(row));
    }

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

    let (id,): (i64,) = sqlx::query_as("SELECT id FROM shopping_items WHERE key = ?")
        .bind(&key)
        .fetch_one(&state.pool)
        .await?;

    let row = fetch_view_by_id(&state, id).await?;
    Ok(Json(row))
}

// PATCH /shopping/{id}
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
        qb.push_bind(if d { 1_i64 } else { 0_i64 });
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

// DELETE /shopping/{id}
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

// POST /shopping/merge
pub async fn merge_items(
    State(state): State<AppState>,
    Json(req): Json<MergeReq>,
) -> AppResult<Json<Vec<ShoppingItemView>>> {
    for it in req.items {
        let mut qty = it.quantity;
        let mut unit = it.unit.map(|u| u.trim().to_lowercase());

        let (qty_from_name, unit_from_name, clean_name) = strip_leading_qty_unit(&it.name);
        let base_name = normalize_name(&clean_name);

        if qty.is_none() {
            qty = qty_from_name;
        }
        if unit.is_none() {
            unit = unit_from_name;
        }

        let (mut unit_norm, qty_norm) = to_canonical_qty_unit(unit.as_deref(), qty);

        if qty_norm.is_none() {
            unit_norm = None;
        }

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
              name = excluded.name,
              unit = excluded.unit,
              category = COALESCE(shopping_items.category, excluded.category)
            "#,
        )
        .bind(&base_name)
        .bind(unit_norm)
        .bind(qty_norm)
        .bind(&key)
        .bind(chosen_cat)
        .execute(&state.pool)
        .await?;
    }

    let rows: Vec<ShoppingItemView> = sqlx::query_as::<_, ShoppingItemView>(
        r#"
        SELECT id, text, done, category
          FROM shopping_items_view
         ORDER BY id
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows))
}
