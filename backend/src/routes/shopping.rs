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
pub async fn create(
    State(state): State<AppState>,
    Json(new): Json<NewItem>,
) -> AppResult<Json<ShoppingItem>> {
    let name_norm = normalize_name(&new.text);
    if name_norm.is_empty() {
        // <-- explicit return with conversion into AppError
        return Err(anyhow::anyhow!("empty shopping item").into());
    }

    let key = make_key(&name_norm, None);

    // Insert if not present (unit/quantity = NULL for plain-text items)
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

    // Return the row in the view shape (id, text, done)
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
        let name_norm = it.name.trim().to_lowercase();
        if name_norm.is_empty() {
            continue;
        }
        let (unit_norm, qty_norm) = to_canonical_unit(it.unit.as_deref(), it.quantity);
        let key = format!("{}|{}", unit_norm.clone().unwrap_or_default(), name_norm);

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
    }

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
