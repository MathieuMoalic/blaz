use crate::categories::validate_category;
use crate::error::AppError;
use crate::ingredients::catalog;
use crate::ingredients::resolver::{OpenRouterFoodLlm, resolve_batch};
use axum::http::StatusCode;
use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;
use sqlx::{QueryBuilder, Sqlite};

use crate::error::AppResult;
use crate::ingredients::parser::parse_ingredient_line;
use crate::models::{AppState, NewItem, ShoppingItemView, ShoppingSource};
use crate::units::{normalize_name, to_storage_qty_unit};

fn internal_err<E: std::fmt::Display>(err: E) -> AppError {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into()
}

fn patch_update_err(err: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(db) = &err
        && db.is_unique_violation()
    {
        return (
            StatusCode::CONFLICT,
            "shopping item with the same name/unit already exists".into(),
        )
            .into();
    }
    internal_err(err)
}

/* ---------- Request/response types ---------- */

#[derive(Deserialize, Debug)]
pub struct UpdateShoppingItem {
    pub done: Option<bool>,
    /// Category change for this item's ingredient.
    ///
    /// For items with Food identity this always teaches the canonical Food
    /// (`foods.category_id`, `category_source = 'user'`); the category then
    /// applies to every shopping row using the same Food. Legacy rows
    /// without a Food keep the per-item override fallback.
    pub category: Option<String>,
    /// Category change by category id; wins over the legacy `category` name.
    #[serde(default)]
    pub category_id: Option<i64>,
    pub notes: Option<String>,

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
    #[serde(default)]
    pub food_id: Option<i64>,
    #[serde(default)]
    pub ingredient_id: Option<String>,
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
}

/* ---------- Line parsing (shared parser adapter) ---------- */

/// Adapter from the shared deterministic parser to the shopping-line shape.
/// Prep wording is intentionally dropped: it never belongs on a shopping row.
fn parse_item_line(raw: &str) -> Option<ParsedItem> {
    let parsed = parse_ingredient_line(raw)?;
    Some(ParsedItem {
        qty: parsed.quantity,
        unit: parsed.unit.map(str::to_string),
        name_raw: parsed.ingredient_phrase,
    })
}

/* ---------- Food-based merge identity ---------- */

/// Unique merge key for food-backed rows: `f:<food_id>|<storage unit>`
/// (`f:42|` for count-style). Aliased spellings of the same food collapse
/// into the same key, so merging never depends on spelling.
fn make_food_key(food_id: i64, unit: Option<&str>) -> String {
    match unit {
        Some(u) if !u.is_empty() => format!("f:{food_id}|{u}"),
        _ => format!("f:{food_id}|"),
    }
}

/// A shopping line resolved to canonical identity + storage units.
struct ShoppingLine {
    food_id: Option<i64>,
    key: String,
    name: String,
    unit: Option<String>,
    quantity: Option<f64>,
    /// Food's category name (from the Food, never guessed per-item).
    category: Option<String>,
}

/// Build the merge key + storage units for an already-known food id.
async fn shopping_line_for_food(
    state: &AppState,
    food_id: Option<i64>,
    name_raw: &str,
    unit: Option<&str>,
    quantity: Option<f64>,
) -> anyhow::Result<ShoppingLine> {
    let food = if let Some(id) = food_id {
        catalog::get_food_by_id(&state.pool, id).await?
    } else {
        None
    };

    let (storage_unit, storage_qty) = to_storage_qty_unit(unit, quantity);
    let storage_unit = storage_unit.map(str::to_string);

    if let Some(food) = food {
        let category = sqlx::query_scalar::<_, String>(
            "SELECT c.name FROM shopping_categories c               JOIN foods f ON f.category_id = c.id              WHERE f.id = ?",
        )
        .bind(food.id)
        .fetch_optional(&state.pool)
        .await?;
        Ok(ShoppingLine {
            food_id: Some(food.id),
            key: make_food_key(food.id, storage_unit.as_deref()),
            name: food.canonical_name.clone(),
            unit: storage_unit,
            quantity: storage_qty,
            category,
        })
    } else {
        let name_norm = normalize_name(name_raw);
        Ok(ShoppingLine {
            food_id: None,
            key: make_key(&name_norm, storage_unit.as_deref()),
            name: name_norm,
            unit: storage_unit,
            quantity: storage_qty,
            category: None,
        })
    }
}

/// Resolve the food identity for a shopping line and compute its merge
/// key + storage units. An explicit `food_id_hint` (client-resolved) wins;
/// otherwise the resolver strategies decide (A–F). Unresolved lines fall
/// back to the legacy name-based key. Category comes from the Food when
/// known, never from a per-item classifier.
async fn resolve_shopping_line(
    state: &AppState,
    food_id_hint: Option<i64>,
    name_raw: &str,
    unit: Option<&str>,
    quantity: Option<f64>,
) -> anyhow::Result<ShoppingLine> {
    let food_id = if let Some(id) = food_id_hint {
        Some(id)
    } else {
        let llm = OpenRouterFoodLlm::from_state(state).await;
        let outcome = resolve_batch(&state.pool, &llm, &[name_raw.to_string()])
            .await
            .map_err(|e| sqlx::Error::Protocol(format!("resolver failed: {e}")))?;
        outcome.first().and_then(|o| o.food_id)
    };
    shopping_line_for_food(state, food_id, name_raw, unit, quantity).await
}

/* ---------- DB helpers ---------- */

const VIEW_COLS: &str = "id, text, done, category, notes, recipe_ids, recipe_titles, \
                          food_id, name, quantity, unit, category_id, category_is_override";

/// Base view columns (sources are attached separately).
#[derive(sqlx::FromRow)]
struct ViewRow {
    id: i64,
    text: String,
    done: i64,
    category: Option<String>,
    notes: String,
    recipe_ids: String,
    recipe_titles: Option<String>,
    food_id: Option<i64>,
    name: Option<String>,
    quantity: Option<f64>,
    unit: Option<String>,
    category_id: Option<i64>,
    category_is_override: bool,
}

impl ViewRow {
    fn into_view(self) -> ShoppingItemView {
        ShoppingItemView {
            id: self.id,
            text: self.text,
            done: self.done,
            category: self.category,
            notes: self.notes,
            recipe_ids: self.recipe_ids,
            recipe_titles: self.recipe_titles,
            food_id: self.food_id,
            name: self.name,
            quantity: self.quantity,
            unit: self.unit,
            category_id: self.category_id,
            category_is_override: self.category_is_override,
            sources: Vec::new(),
        }
    }
}

type SourceRow = (
    i64,
    String,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<f64>,
    Option<String>,
);

/// Attach recorded contributions to shopping rows.
async fn attach_sources(
    state: &AppState,
    items: &mut [ShoppingItemView],
) -> Result<(), sqlx::Error> {
    if items.is_empty() {
        return Ok(());
    }
    let ids: Vec<i64> = items.iter().map(|i| i.id).collect();
    let mut qb = QueryBuilder::<Sqlite>::new(
        "SELECT s.shopping_item_id, s.source_type, s.recipe_id, r.title, \
                                     s.recipe_ingredient_id, s.quantity, s.unit \
                                     FROM shopping_item_sources s \
                                     LEFT JOIN recipes r ON r.id = s.recipe_id \
                                     WHERE s.shopping_item_id IN (",
    );
    let mut separated = qb.separated(", ");
    for id in &ids {
        separated.push_bind(*id);
    }
    separated.push_unseparated(")");

    let rows: Vec<SourceRow> = qb.build_query_as().fetch_all(&state.pool).await?;

    let mut grouped: std::collections::HashMap<i64, Vec<ShoppingSource>> =
        std::collections::HashMap::new();
    for (item_id, source_type, recipe_id, recipe_title, recipe_ingredient_id, quantity, unit) in
        rows
    {
        grouped.entry(item_id).or_default().push(ShoppingSource {
            source_type,
            recipe_id,
            recipe_title,
            recipe_ingredient_id,
            quantity,
            unit,
        });
    }
    for item in items {
        item.sources = grouped.remove(&item.id).unwrap_or_default();
    }
    Ok(())
}

async fn fetch_view_by_id(state: &AppState, id: i64) -> Result<ShoppingItemView, sqlx::Error> {
    let sql = format!("SELECT {VIEW_COLS} FROM shopping_items_view WHERE id = ?");
    let row: ViewRow = sqlx::query_as(&sql).bind(id).fetch_one(&state.pool).await?;
    let mut view = row.into_view();
    attach_sources(state, std::slice::from_mut(&mut view)).await?;
    Ok(view)
}

/// Record a contribution for a shopping row (recipe or manual add).
async fn record_source(
    state: &AppState,
    item_id: i64,
    source_type: &str,
    recipe_id: Option<i64>,
    recipe_ingredient_id: Option<&str>,
    quantity: Option<f64>,
    unit: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO shopping_item_sources \
         (shopping_item_id, source_type, recipe_id, recipe_ingredient_id, quantity, unit) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(item_id)
    .bind(source_type)
    .bind(recipe_id)
    .bind(recipe_ingredient_id)
    .bind(quantity)
    .bind(unit)
    .execute(&state.pool)
    .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ShoppingItemRow {
    name: String,
    unit: Option<String>,
    quantity: Option<f64>,
    done: i64,
    category: Option<String>,
    notes: String,
    recipe_ids: String,
    food_id: Option<i64>,
}

async fn fetch_raw_by_id(state: &AppState, id: i64) -> Result<ShoppingItemRow, sqlx::Error> {
    sqlx::query_as::<_, ShoppingItemRow>(
        r"
        SELECT
            name,
            unit,
            quantity,
            done,
            category,
            notes,
            COALESCE(recipe_ids, '[]') AS recipe_ids,
            food_id
          FROM shopping_items
         WHERE id = ?
        ",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
}

async fn category_id_exists(state: &AppState, id: i64) -> bool {
    sqlx::query_scalar::<_, i64>("SELECT id FROM shopping_categories WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten()
        .is_some()
}

async fn category_id_by_name(state: &AppState, name: &str) -> Option<i64> {
    sqlx::query_scalar("SELECT id FROM shopping_categories WHERE name = ?")
        .bind(name)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten()
}

async fn category_name_by_id(state: &AppState, id: Option<i64>) -> Option<String> {
    let cid = id?;
    sqlx::query_scalar("SELECT name FROM shopping_categories WHERE id = ?")
        .bind(cid)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten()
}

#[derive(Debug)]
struct ResolvedPatch {
    name: String,
    unit: Option<String>,
    quantity: Option<f64>,
    done: bool,
    category: Option<String>,
    notes: String,
    recipe_ids: String,
    key: String,
    food_id: Option<i64>,
}

fn merge_recipe_ids_json(existing: &str, incoming: &str) -> String {
    let mut ids = Vec::<i64>::new();
    let mut push_ids = |src: &str| {
        let Ok(values) = serde_json::from_str::<Vec<serde_json::Value>>(src) else {
            return;
        };
        for v in values {
            if let Some(id) = v.as_i64()
                && !ids.contains(&id)
            {
                ids.push(id);
            }
        }
    };

    push_ids(existing);
    push_ids(incoming);
    serde_json::to_string(&ids).unwrap_or_else(|_| "[]".to_string())
}

fn merge_quantities(existing: Option<f64>, incoming: Option<f64>) -> Option<f64> {
    match (existing, incoming) {
        (Some(a), Some(b)) => Some(a + b),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

#[allow(clippy::too_many_lines)]
async fn resolve_patch_values(
    state: &AppState,
    id: i64,
    payload: &UpdateShoppingItem,
) -> AppResult<ResolvedPatch> {
    let current = fetch_raw_by_id(state, id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let done = payload.done.unwrap_or(current.done != 0);
    let notes = payload
        .notes
        .clone()
        .unwrap_or_else(|| current.notes.clone());

    let category = match payload.category.as_ref() {
        Some(c) => {
            let c = crate::units::norm_whitespace(c);
            if c.is_empty() {
                None
            } else if validate_category(state, &c).await {
                Some(c)
            } else {
                return Err((StatusCode::BAD_REQUEST, "invalid category".into()).into());
            }
        }
        None => current.category.clone(),
    };

    if let Some(t) = payload.text.as_deref() {
        let parsed =
            parse_item_line(t).ok_or_else(|| (StatusCode::BAD_REQUEST, "empty text".into()))?;
        let line = resolve_shopping_line(
            state,
            None,
            &parsed.name_raw,
            parsed.unit.as_deref(),
            parsed.qty,
        )
        .await
        .map_err(internal_err)?;

        let category = if payload.category.is_none() {
            line.category.clone().or(current.category)
        } else {
            category
        };

        return Ok(ResolvedPatch {
            name: line.name,
            unit: line.unit,
            quantity: line.quantity,
            done,
            category,
            notes,
            recipe_ids: current.recipe_ids,
            key: line.key,
            food_id: line.food_id,
        });
    }

    let has_structured =
        payload.name.is_some() || payload.unit.is_some() || payload.quantity.is_some();
    if !has_structured {
        let key = match current.food_id {
            Some(fid) => make_food_key(fid, current.unit.as_deref()),
            None => make_key(&current.name, current.unit.as_deref()),
        };
        return Ok(ResolvedPatch {
            name: current.name,
            unit: current.unit,
            quantity: current.quantity,
            done,
            category,
            notes,
            recipe_ids: current.recipe_ids,
            key,
            food_id: current.food_id,
        });
    }

    let new_name_raw = if let Some(n) = payload.name.clone() {
        let n = n.trim().to_string();
        if n.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "empty name".into()).into());
        }
        n
    } else {
        current.name.clone()
    };

    let new_unit_raw = payload.unit.clone().map(|u| u.trim().to_string());
    let new_unit_raw = match new_unit_raw.as_deref() {
        Some("") => None,
        Some(u) => Some(u.to_string()),
        None => current.unit.clone(),
    };

    let new_qty = payload.quantity.or(current.quantity);

    // A name change re-resolves identity; quantity/unit-only edits keep it.
    let line = if payload.name.is_some() {
        resolve_shopping_line(state, None, &new_name_raw, new_unit_raw.as_deref(), new_qty)
            .await
            .map_err(internal_err)?
    } else {
        let (storage_unit, storage_qty) = to_storage_qty_unit(new_unit_raw.as_deref(), new_qty);
        let storage_unit = storage_unit.map(str::to_string);
        let key = match current.food_id {
            Some(fid) => make_food_key(fid, storage_unit.as_deref()),
            None => make_key(&normalize_name(&current.name), storage_unit.as_deref()),
        };
        ShoppingLine {
            food_id: current.food_id,
            key,
            name: current.name.clone(),
            unit: storage_unit,
            quantity: storage_qty,
            category: current.category.clone(),
        }
    };

    let category = if payload.category.is_none() {
        line.category.clone().or(current.category)
    } else {
        category
    };

    Ok(ResolvedPatch {
        name: line.name,
        unit: line.unit,
        quantity: line.quantity,
        done,
        category,
        notes,
        recipe_ids: current.recipe_ids,
        key: line.key,
        food_id: line.food_id,
    })
}

async fn resolve_patch_conflict(
    state: &AppState,
    id: i64,
    payload: &UpdateShoppingItem,
) -> AppResult<Json<ShoppingItemView>> {
    let resolved = resolve_patch_values(state, id, payload).await?;
    let Some((
        conflict_id,
        conflict_quantity,
        _conflict_done,
        conflict_category,
        conflict_notes,
        conflict_recipe_ids,
        conflict_food_id,
    )) = sqlx::query_as::<
        _,
        (
            i64,
            Option<f64>,
            i64,
            Option<String>,
            String,
            String,
            Option<i64>,
        ),
    >(
        r"
        SELECT id,
               quantity,
               done,
               category,
               notes,
               COALESCE(recipe_ids, '[]') AS recipe_ids,
               food_id
          FROM shopping_items
         WHERE key = ? AND id != ?
        ",
    )
    .bind(&resolved.key)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    else {
        return Err((
            StatusCode::CONFLICT,
            "shopping item with the same name/unit already exists".into(),
        )
            .into());
    };

    let merged_recipe_ids = merge_recipe_ids_json(&conflict_recipe_ids, &resolved.recipe_ids);
    let merged_quantity = merge_quantities(conflict_quantity, resolved.quantity);
    let merged_done = resolved.done;
    let merged_category = conflict_category.or(resolved.category);
    let merged_food_id = resolved.food_id.or(conflict_food_id);
    let merged_notes = if resolved.notes.is_empty() {
        conflict_notes
    } else {
        resolved.notes
    };

    sqlx::query(
        r"
        UPDATE shopping_items
           SET name = ?,
               unit = ?,
               quantity = ?,
               done = ?,
               category = ?,
               notes = ?,
               recipe_ids = ?,
               food_id = ?
         WHERE id = ?
        ",
    )
    .bind(&resolved.name)
    .bind(&resolved.unit)
    .bind(merged_quantity)
    .bind(i64::from(merged_done))
    .bind(&merged_category)
    .bind(&merged_notes)
    .bind(&merged_recipe_ids)
    .bind(merged_food_id)
    .bind(conflict_id)
    .execute(&state.pool)
    .await
    .map_err(internal_err)?;

    sqlx::query("DELETE FROM shopping_items WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal_err)?;

    let dto = fetch_view_by_id(state, conflict_id)
        .await
        .map_err(internal_err)?;
    Ok(Json(dto))
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
/// Returns ONLY non-done items.
/// Done items are kept in DB so their unit/category data remains for future edits.
///
/// # Errors
/// Err if querying the database fails.
/// DELETE-grade subtraction: `POST /shopping/sources/remove`
#[derive(Deserialize)]
pub struct RemoveSourcesReq {
    pub recipe_id: i64,
}

/// `POST /shopping/sources/remove`
///
/// Removes one recipe's contributions from the list and recomputes each
/// affected row's quantity from its remaining sources (rows left with no
/// sources are deleted). Other recipes' contributions stay intact.
///
/// # Errors
///
/// Err if a database operation fails.
pub async fn remove_recipe_sources(
    State(state): State<AppState>,
    Json(req): Json<RemoveSourcesReq>,
) -> AppResult<Json<Vec<ShoppingItemView>>> {
    let affected_items: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT shopping_item_id FROM shopping_item_sources WHERE recipe_id = ?",
    )
    .bind(req.recipe_id)
    .fetch_all(&state.pool)
    .await?;

    sqlx::query("DELETE FROM shopping_item_sources WHERE recipe_id = ?")
        .bind(req.recipe_id)
        .execute(&state.pool)
        .await?;

    for item_id in affected_items {
        let remaining: Option<f64> = sqlx::query_scalar(
            "SELECT SUM(quantity) FROM shopping_item_sources WHERE shopping_item_id = ?",
        )
        .bind(item_id)
        .fetch_one(&state.pool)
        .await?;

        match remaining {
            None | Some(0.0) => {
                sqlx::query("DELETE FROM shopping_items WHERE id = ?")
                    .bind(item_id)
                    .execute(&state.pool)
                    .await?;
            }
            Some(total) => {
                sqlx::query(
                    "UPDATE shopping_items SET quantity = ?, recipe_ids = ( \
                       SELECT json_group_array(DISTINCT recipe_id) \
                       FROM shopping_item_sources \
                       WHERE shopping_item_id = ? AND recipe_id IS NOT NULL \
                     ) WHERE id = ?",
                )
                .bind(total)
                .bind(item_id)
                .bind(item_id)
                .execute(&state.pool)
                .await?;
            }
        }
    }

    list(State(state)).await
}

pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<ShoppingItemView>>> {
    let sql = format!("SELECT {VIEW_COLS} FROM shopping_items_view WHERE done = 0 ORDER BY id");
    let rows: Vec<ViewRow> = sqlx::query_as(&sql).fetch_all(&state.pool).await?;
    let mut rows: Vec<ShoppingItemView> = rows.into_iter().map(ViewRow::into_view).collect();
    attach_sources(&state, &mut rows).await?;

    // Nicer ordering: user's category order, then insertion order.
    // Rows without any category sort last (they display as Uncategorized).
    let orders: std::collections::HashMap<String, i64> =
        sqlx::query_as("SELECT name, sort_order FROM shopping_categories")
            .fetch_all(&state.pool)
            .await?
            .into_iter()
            .collect();
    rows.sort_by_key(|r| {
        let cat_key = r
            .category
            .as_deref()
            .and_then(|c| orders.get(c))
            .copied()
            .unwrap_or_else(|| i64::from(u16::MAX));
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
    if text.is_empty() && new.food_id.is_none() {
        return Err(StatusCode::BAD_REQUEST.into());
    }

    let line = if let Some(food_id) = new.food_id {
        if catalog::get_food_by_id(&state.pool, food_id)
            .await
            .map_err(internal_err)?
            .is_none()
        {
            return Err((StatusCode::BAD_REQUEST, "unknown food".into()).into());
        }
        shopping_line_for_food(&state, Some(food_id), "", new.unit.as_deref(), new.quantity)
            .await
            .map_err(internal_err)?
    } else {
        let parsed = parse_item_line(text).ok_or(StatusCode::BAD_REQUEST)?;
        resolve_shopping_line(
            &state,
            None,
            &parsed.name_raw,
            parsed.unit.as_deref(),
            parsed.qty,
        )
        .await
        .map_err(internal_err)?
    };

    sqlx::query(
        r"
        INSERT INTO shopping_items (name, unit, quantity, done, key, category, food_id)
        VALUES (?, ?, ?, 0, ?, ?, ?)
        ON CONFLICT(key) DO UPDATE SET
          quantity = CASE
            WHEN shopping_items.done = 1 THEN excluded.quantity
            WHEN excluded.quantity IS NULL THEN shopping_items.quantity
            WHEN shopping_items.quantity IS NULL THEN excluded.quantity
            ELSE shopping_items.quantity + excluded.quantity
          END,
          name = excluded.name,
          unit = excluded.unit,
          food_id = COALESCE(excluded.food_id, shopping_items.food_id),
          category = COALESCE(shopping_items.category, excluded.category),
          done = 0
        ",
    )
    .bind(&line.name)
    .bind(&line.unit)
    .bind(line.quantity)
    .bind(&line.key)
    .bind(&line.category)
    .bind(line.food_id)
    .execute(&state.pool)
    .await?;

    let (id,): (i64,) = sqlx::query_as("SELECT id FROM shopping_items WHERE key = ?")
        .bind(&line.key)
        .fetch_one(&state.pool)
        .await?;

    record_source(
        &state,
        id,
        "manual",
        None,
        None,
        line.quantity,
        line.unit.as_deref(),
    )
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

        // Clear recipe_ids, quantity and notes when marking as done so list resets cleanly
        if d {
            push_sep(qb, wrote);
            qb.push("recipe_ids = '[]'");
            push_sep(qb, wrote);
            qb.push("quantity = NULL");
            push_sep(qb, wrote);
            qb.push("notes = ''");
        }
    }
}

fn apply_notes_update(qb: &mut QueryBuilder<Sqlite>, wrote: &mut bool, notes: Option<String>) {
    if let Some(n) = notes {
        push_sep(qb, wrote);
        qb.push("notes = ");
        qb.push_bind(n);
    }
}

async fn apply_category_update(
    qb: &mut QueryBuilder<'_, Sqlite>,
    wrote: &mut bool,
    state: &AppState,
    id: i64,
    payload: &UpdateShoppingItem,
) -> AppResult<()> {
    // Resolve the desired category: an explicit `category_id` wins over the
    // legacy `category` *name*; an empty name clears it.
    let category_id = match (payload.category_id, payload.category.as_deref()) {
        (Some(cid), _) => {
            if category_id_exists(state, cid).await {
                Some(cid)
            } else {
                return Err((StatusCode::BAD_REQUEST, "invalid category".into()).into());
            }
        }
        (None, Some(name)) => {
            let name = crate::units::norm_whitespace(name);
            if name.is_empty() {
                None // clear the override
            } else if let Some(cid) = category_id_by_name(state, &name).await {
                Some(cid)
            } else {
                return Err((StatusCode::BAD_REQUEST, "invalid category".into()).into());
            }
        }
        (None, None) => return Ok(()), // untouched
    };

    let name = category_name_by_id(state, category_id).await;

    // Items with Food identity: the change always teaches the canonical
    // Food (user-locked), then the item row follows the Food. One-time
    // overrides are cleared so the Food's choice is authoritative.
    let food_id: Option<i64> =
        sqlx::query_scalar("SELECT food_id FROM shopping_items WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await
            .map_err(internal_err)?
            .flatten();

    if let Some(fid) = food_id
        && let Some(cid) = category_id
    {
        crate::ingredients::catalog::set_food_category(
            &state.pool,
            fid,
            Some(cid),
            "user",
            None,
            true,
        )
        .await
        .map_err(|e| -> AppError { (StatusCode::BAD_REQUEST, e.to_string()).into() })?;
    }

    push_sep(qb, wrote);
    if food_id.is_some() {
        // Follow the Food: clear any legacy per-item override.
        qb.push("category_override_id = NULL");
    } else {
        // Legacy row without Food identity: per-item fallback stays.
        qb.push("category_override_id = ");
        if let Some(cid) = category_id {
            qb.push_bind(cid);
        } else {
            qb.push("NULL");
        }
    }

    push_sep(qb, wrote);
    qb.push("category = ");
    if let Some(n) = name {
        qb.push_bind(n);
    } else {
        qb.push("NULL");
    }

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

    let line = resolve_shopping_line(
        state,
        None,
        &parsed.name_raw,
        parsed.unit.as_deref(),
        parsed.qty,
    )
    .await
    .map_err(internal_err)?;

    push_sep(qb, wrote);

    qb.push("name = ");
    qb.push_bind(line.name);

    qb.push(", quantity = ");
    if let Some(q) = line.quantity {
        qb.push_bind(q);
    } else {
        qb.push("NULL");
    }

    qb.push(", unit = ");
    if let Some(u) = line.unit.clone() {
        qb.push_bind(u);
    } else {
        qb.push("NULL");
    }

    qb.push(", key = ");
    qb.push_bind(line.key.clone());

    qb.push(", food_id = ");
    if let Some(f) = line.food_id {
        qb.push_bind(f);
    } else {
        qb.push("NULL");
    }

    // Without an explicit category, the Food's category applies when known;
    // otherwise the existing value stays (never a per-item guess).
    if payload.category.is_none() {
        qb.push(", category = COALESCE(");
        qb.push_bind(line.category);
        qb.push(", category)");
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

    let new_unit_raw = payload.unit.clone().map(|u| u.trim().to_string());
    let new_unit_raw = match new_unit_raw.as_deref() {
        Some("") => None, // allow clearing
        Some(u) => Some(u.to_string()),
        None => current.unit.clone(),
    };

    let new_qty = payload.quantity.or(current.quantity);

    // A name change re-resolves identity; a quantity/unit-only edit keeps
    // the existing food and only re-canonicalizes the storage units.
    let line = if payload.name.is_some() {
        resolve_shopping_line(state, None, &new_name_raw, new_unit_raw.as_deref(), new_qty)
            .await
            .map_err(internal_err)?
    } else {
        let (storage_unit, storage_qty) = to_storage_qty_unit(new_unit_raw.as_deref(), new_qty);
        let storage_unit = storage_unit.map(str::to_string);
        let key = match current.food_id {
            Some(fid) => make_food_key(fid, storage_unit.as_deref()),
            None => make_key(&normalize_name(&current.name), storage_unit.as_deref()),
        };
        ShoppingLine {
            food_id: current.food_id,
            key,
            name: current.name.clone(),
            unit: storage_unit,
            quantity: storage_qty,
            category: current.category.clone(),
        }
    };

    push_sep(qb, wrote);

    qb.push("name = ");
    qb.push_bind(line.name);

    qb.push(", quantity = ");
    if let Some(q) = line.quantity {
        qb.push_bind(q);
    } else {
        qb.push("NULL");
    }

    qb.push(", unit = ");
    if let Some(u) = line.unit.clone() {
        qb.push_bind(u);
    } else {
        qb.push("NULL");
    }

    qb.push(", key = ");
    qb.push_bind(line.key.clone());

    qb.push(", food_id = ");
    if let Some(f) = line.food_id {
        qb.push_bind(f);
    } else {
        qb.push("NULL");
    }

    // Without an explicit category: a re-resolved name takes the Food's
    // category when known; otherwise the existing value stays.
    if payload.category.is_none() {
        qb.push(", category = COALESCE(");
        qb.push_bind(line.category);
        qb.push(", category)");
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
    apply_category_update(&mut qb, &mut wrote, &state, id, &payload).await?;
    apply_notes_update(&mut qb, &mut wrote, payload.notes.clone());

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

    let rid = match qb.build_query_as::<(i64,)>().fetch_one(&state.pool).await {
        Ok((rid,)) => rid,
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            return resolve_patch_conflict(&state, id, &payload).await;
        }
        Err(err) => return Err(patch_update_err(err)),
    };

    // Marking an item done resets for the next trip: clear its history.
    if payload.done == Some(true) {
        sqlx::query("DELETE FROM shopping_item_sources WHERE shopping_item_id = ?")
            .bind(rid)
            .execute(&state.pool)
            .await
            .map_err(internal_err)?;
    }

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
    // Resolve all items in one batch (at most one LLM call per request).
    let llm = OpenRouterFoodLlm::from_state(&state).await;
    let names: Vec<String> = req.items.iter().map(|it| it.name.clone()).collect();
    let outcomes = resolve_batch(&state.pool, &llm, &names)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(?e, "shopping merge resolution failed; using legacy keys");
            std::iter::repeat_with(crate::ingredients::types::ResolutionOutcome::unresolved)
                .take(names.len())
                .collect()
        });

    for (it, outcome) in req.items.iter().zip(&outcomes) {
        let line = shopping_line_for_food(
            &state,
            it.food_id.or(outcome.food_id),
            &it.name,
            it.unit.as_deref(),
            it.quantity,
        )
        .await
        .map_err(internal_err)?;

        // Explicit incoming category (by name) wins; else the Food's.
        let chosen_cat = match it.category.as_ref() {
            Some(c) => {
                let c = crate::units::norm_whitespace(c);
                if c.is_empty() {
                    None
                } else if validate_category(&state, &c).await {
                    Some(c)
                } else {
                    return Err((StatusCode::BAD_REQUEST, "invalid category".into()).into());
                }
            }
            None => None,
        };

        // Prepare recipe_ids JSON array
        let recipe_ids_json = req
            .recipe_id
            .map_or_else(|| "[]".to_string(), |rid| format!("[{rid}]"));

        sqlx::query(
            r"
            INSERT INTO shopping_items (name, unit, quantity, done, key, category, recipe_ids, food_id)
            VALUES (?, ?, ?, 0, ?, ?, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
              quantity = CASE
                WHEN excluded.quantity IS NULL THEN shopping_items.quantity
                WHEN shopping_items.quantity IS NULL THEN excluded.quantity
                ELSE shopping_items.quantity + excluded.quantity
              END,
              name = excluded.name,
              unit = excluded.unit,
              food_id = COALESCE(excluded.food_id, shopping_items.food_id),
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
        .bind(&line.name)
        .bind(&line.unit)
        .bind(line.quantity)
        .bind(&line.key)
        .bind(chosen_cat.or_else(|| line.category.clone()))
        .bind(&recipe_ids_json)
        .bind(line.food_id)
        .execute(&state.pool)
        .await?;

        // Record the exact contribution (recipe provenance).
        if let Some(recipe_id) = req.recipe_id {
            let (item_id,): (i64,) = sqlx::query_as("SELECT id FROM shopping_items WHERE key = ?")
                .bind(&line.key)
                .fetch_one(&state.pool)
                .await?;
            record_source(
                &state,
                item_id,
                "recipe",
                Some(recipe_id),
                it.ingredient_id.as_deref(),
                it.quantity,
                it.unit.as_deref(),
            )
            .await?;
        }
    }

    // Return the active (not done) list
    list(State(state)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_name_basic() {
        assert_eq!(normalize_name("flour"), "flour");
        assert_eq!(normalize_name("  Flour  "), "flour");
        assert_eq!(normalize_name("olive oil"), "olive oil");
    }

    #[test]
    fn test_normalize_name_strips_punctuation() {
        // hyphens and apostrophes should be preserved by normalize_name
        let n = normalize_name("all-purpose flour");
        assert!(!n.is_empty());
    }

    #[test]
    fn test_merge_key_same_name_different_units_are_separate() {
        // "1 kg potatoes" and "3 potatoes" must NOT merge — different units
        let key1 = make_key("potatoes", Some("kg"));
        let key2 = make_key("potatoes", None);
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_merge_key_same_name_same_unit_are_equal() {
        let key1 = make_key("potatoes", Some("kg"));
        let key2 = make_key("potatoes", Some("kg"));
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_merge_key_name_is_normalized() {
        let key1 = make_key(&normalize_name("Flour"), None);
        let key2 = make_key(&normalize_name("flour"), None);
        assert_eq!(key1, key2);
    }
}
