//! Ingredient pipeline: shared resolution pass for every save path.
//!
//! All recipes (URL/photo import, `RecipeSage`, manual, edits) funnel through
//! [`ensure_resolved`]: `raw` lines are structured with the deterministic
//! parser, stable ingredient IDs are assigned, and every non-section
//! ingredient goes through the semantic Food resolver. Recipe-visible
//! wording is never rewritten; only identity metadata is attached.

use crate::error::AppError;
use crate::error::AppResult;
use crate::ingredients::catalog;
use crate::ingredients::parser::parse_ingredient_line;
use crate::ingredients::resolver::{self, OpenRouterFoodLlm};
use crate::models::{AppState, Ingredient};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

/* =========================
 * POST /foods/{id}/aliases
 * ========================= */

#[derive(Deserialize, Debug)]
pub struct ConfirmAliasReq {
    /// The ingredient wording to lock to this Food, e.g. "chinese parsley".
    pub alias: String,
}

/// `POST /foods/{id}/aliases`
///
/// User-confirms that `alias` means this Food (`source = 'user'`,
/// `confirmed = 1`). The resolver never overwrites it automatically — this
/// is how user corrections teach the system (§29).
///
/// # Errors
///
/// `404` if the food does not exist; `400` for an empty alias.
pub async fn confirm_food_alias(
    State(state): State<AppState>,
    Path(food_id): Path<i64>,
    Json(req): Json<ConfirmAliasReq>,
) -> AppResult<Json<crate::ingredients::types::FoodAlias>> {
    let alias = catalog::confirm_alias(&state.pool, &req.alias, food_id)
        .await
        .map_err(|e| -> AppError {
            if e.to_string().contains("does not exist") {
                (StatusCode::NOT_FOUND, e.to_string()).into()
            } else {
                (StatusCode::BAD_REQUEST, e.to_string()).into()
            }
        })?;
    Ok(Json(alias))
}

/* =========================
 * PATCH /foods/{id}
 * ========================= */

#[derive(Deserialize, Debug, Default)]
pub struct UpdateFoodReq {
    /// Food's default category (persisted as a user choice).
    #[serde(default)]
    pub category_id: Option<i64>,
    /// Optional canonical name rename.
    #[serde(default)]
    pub canonical_name: Option<String>,
}

/// `PATCH /foods/{id}` — change a Food's default category or rename it.
///
/// Category changes are stored with `category_source = 'user'` and are never
/// overwritten by automatic resolution afterwards.
///
/// # Errors
///
/// `404` if the food does not exist; `400` for unknown categories or empty
/// names; `409` if the new name collides with an existing Food.
pub async fn update_food(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateFoodReq>,
) -> AppResult<Json<crate::ingredients::types::Food>> {
    if catalog::get_food_by_id(&state.pool, id).await?.is_none() {
        return Err(StatusCode::NOT_FOUND.into());
    }

    if let Some(name) = req.canonical_name {
        let name = name.trim();
        if name.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "empty food name".into()).into());
        }
        let normalized = crate::units::normalize_name(name);
        if normalized.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "invalid food name".into()).into());
        }
        let res = match sqlx::query(
            "UPDATE foods SET canonical_name = ?, normalized_name = ?, updated_at = unixepoch() \
              WHERE id = ?",
        )
        .bind(name)
        .bind(&normalized)
        .bind(id)
        .execute(&state.pool)
        .await
        {
            Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
                return Err((StatusCode::CONFLICT, "food name already exists".into()).into());
            }
            Err(e) => return Err(e.into()),
            Ok(res) => res,
        };
        if res.rows_affected() == 0 {
            return Err(StatusCode::NOT_FOUND.into());
        }
    }

    if let Some(category_id) = req.category_id {
        let known: Option<i64> =
            sqlx::query_scalar("SELECT id FROM shopping_categories WHERE id = ?")
                .bind(category_id)
                .fetch_optional(&state.pool)
                .await?;
        if known.is_none() {
            return Err((StatusCode::BAD_REQUEST, "unknown category".into()).into());
        }
        catalog::set_food_category(&state.pool, id, Some(category_id), "user", None, true).await?;
    }

    Ok(Json(
        catalog::get_food_by_id(&state.pool, id)
            .await?
            .ok_or(StatusCode::NOT_FOUND)?,
    ))
}

/* =========================
 * GET /foods
 * ========================= */

#[derive(Deserialize, Debug, Default)]
pub struct FoodSearchQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default = "default_food_limit")]
    pub limit: usize,
}

const fn default_food_limit() -> usize {
    20
}

/// `GET /foods?q=pot&limit=20`
///
/// Search canonical food names and aliases (case-insensitive substring).
/// Powers shopping autocomplete and recipe-edit food pickers.
///
/// # Errors
///
/// Err if the database lookup fails.
pub async fn search_foods(
    State(state): State<AppState>,
    Query(query): Query<FoodSearchQuery>,
) -> AppResult<Json<Vec<crate::ingredients::types::FoodSearchRow>>> {
    let rows = catalog::search_foods(&state.pool, &query.q, query.limit).await?;
    Ok(Json(rows))
}

/* =========================
 * POST /ingredients/resolve
 * ========================= */

#[derive(Deserialize, Debug)]
pub struct ResolveLinesReq {
    /// Free-form ingredient lines, e.g. "1/2 teaspoon cumin".
    #[serde(default)]
    pub lines: Vec<String>,
}

#[derive(Serialize)]
pub struct ResolveLinesResp {
    pub ingredients: Vec<Ingredient>,
}

/// `POST /ingredients/resolve`
///
/// The shared entry point for manual/free-form input: each line is parsed
/// with the deterministic parser (fractions, units, prep — no LLM needed)
/// and resolved to stable Food identity. Unresolvable lines come back
/// flagged `needs_review` instead of blocking the caller.
///
/// # Errors
///
/// Err if the request body is malformed.
pub async fn resolve_lines(
    State(state): State<AppState>,
    Json(req): Json<ResolveLinesReq>,
) -> AppResult<Json<ResolveLinesResp>> {
    let mut ingredients: Vec<Ingredient> = req
        .lines
        .iter()
        .filter_map(|line| parse_ingredient_line(line))
        .map(|parsed| Ingredient::from_parsed(&parsed))
        .collect();

    if let Err(e) = ensure_resolved(&state, &mut ingredients).await {
        tracing::warn!(?e, "ingredient resolution failed");
    }

    Ok(Json(ResolveLinesResp { ingredients }))
}

/// Structure raw lines and assign stable food identity to all ingredients.
///
/// - section headers pass through untouched;
/// - `raw` ingredients are upgraded with the deterministic parser
///   (quantity/unit/name/prep), never left as unparsed text;
/// - missing `ingredient_id`s are generated;
/// - every non-section ingredient is resolved through the resolver
///   strategies (A–F); ingredients resolved to nothing with no prior
///   identity are flagged `needs_review` — resolution never blocks saving.
///
/// # Errors
///
/// Returns an error if the database lookup fails. LLM failures are
/// contained inside the resolver and only degrade resolution quality.
pub async fn ensure_resolved(
    state: &AppState,
    ingredients: &mut [Ingredient],
) -> anyhow::Result<()> {
    // 1) Structure raw lines deterministically and assign instance IDs.
    for ing in ingredients.iter_mut() {
        if ing.section.is_some() || ing.name.trim().is_empty() {
            continue;
        }
        if ing.raw
            && let Some(parsed) = parse_ingredient_line(&ing.name)
        {
            ing.quantity = parsed.quantity;
            ing.unit = parsed.unit.map(str::to_string);
            if !parsed.ingredient_phrase.is_empty() {
                ing.name.clone_from(&parsed.ingredient_phrase);
            }
            if ing.prep.is_none() {
                ing.prep = parsed.prep;
            }
            if ing.raw_text.is_none() {
                ing.raw_text = Some(parsed.raw_text.clone());
            }
            ing.raw = false;
        }
        if ing.ingredient_id.is_none() {
            ing.ingredient_id = Some(uuid::Uuid::new_v4().to_string());
        }
    }

    // 2) Resolve semantic identity for all real ingredients.
    let mut indices: Vec<usize> = Vec::new();
    let mut phrases: Vec<String> = Vec::new();
    for (i, ing) in ingredients.iter().enumerate() {
        if ing.section.is_some() {
            continue;
        }
        indices.push(i);
        phrases.push(ing.name.clone());
    }
    if phrases.is_empty() {
        return Ok(());
    }

    let llm = OpenRouterFoodLlm::from_state(state).await;
    let outcomes = resolver::resolve_batch(&state.pool, &llm, &phrases).await?;

    for (i, outcome) in indices.into_iter().zip(outcomes) {
        let ing = &mut ingredients[i];
        if outcome.food_id.is_some() {
            ing.food_id = outcome.food_id;
            ing.canonical_name = outcome.canonical_name;
            if !outcome.qualifiers.is_empty() {
                ing.qualifiers = outcome.qualifiers;
            }
            ing.resolution_source = outcome.resolution_source.map(str::to_string);
            ing.resolution_confidence = outcome.resolution_confidence;
            ing.needs_review = outcome.needs_review;
        } else if ing.food_id.is_none() {
            // Nothing resolved and no prior identity: flag for review.
            ing.needs_review = true;
            ing.resolution_source = Some("unresolved".to_string());
        }
        // A client-resolved food_id the resolver can't confirm stays trusted.
    }

    Ok(())
}
