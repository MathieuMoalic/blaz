//! Idempotent ingredient backfill.
//!
//! `blaz backfill-ingredients` resolves legacy recipes and shopping rows to
//! stable Food identity: distinct ingredient names are resolved once each
//! (deterministically first, via the batched LLM afterwards), then
//! `food_id` is written back to the shopping rows and recipe JSON.
//!
//! By default the command only processes ingredients that were never
//! attempted: already-resolved entries are reused and needs-review /
//! unresolved entries are **skipped** (no LLM calls, no new Foods). Pass
//! `--retry-unresolved` to re-attempt them after resolver improvements.
//!
//! Legacy ingredient text is run through the shared deterministic parser
//! before semantic resolution, so quantity/unit/preparation wording (and
//! price annotations like `($0.24)`) never reach the Food resolver.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use sqlx::SqlitePool;

use crate::config::Config;
use crate::ingredients::parser::parse_ingredient_line;
use crate::ingredients::resolver::{OpenRouterFoodLlm, resolve_batch};
use crate::ingredients::types::ResolutionOutcome;
use crate::models::Ingredient;
use crate::routes::settings::LlmSettings;
use crate::units::normalize_name;

/// Price annotations like "($0.24)" must not leak into Food identity.
static PRICE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*\(\$[^)]*\)").unwrap());

/// Counters for the final summary.
#[derive(Default, Debug)]
pub struct BackfillStats {
    pub recipes_scanned: u64,
    pub ingredients_scanned: u64,
    pub resolved_locally: u64,
    pub resolved_via_llm: u64,
    pub needs_review: u64,
    pub unresolved: u64,
    pub foods_created: u64,
    pub aliases_created: u64,
    pub ingredients_updated: u64,
    pub shopping_updated: u64,
    pub skipped_attempted: u64,
}

/// Load every recipe's ingredient list for backfilling.
async fn load_recipe_ingredients(
    pool: &SqlitePool,
) -> anyhow::Result<Vec<(i64, Vec<Ingredient>)>> {
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, ingredients FROM recipes").fetch_all(pool).await?;
    let mut out = Vec::with_capacity(rows.len());
    for (id, json) in rows {
        let ings: Vec<Ingredient> = serde_json::from_str(&json).unwrap_or_default();
        out.push((id, ings));
    }
    Ok(out)
}

/// Whether an ingredient has already been handled and must not be touched
/// again on a default run.
///
/// - resolved (has a Food and no review flag): reuse, never re-resolve;
/// - needs-review or explicitly unresolved: already attempted — skipped
///   unless `retry_unresolved` is set.
fn is_attempted(ing: &Ingredient, retry_unresolved: bool) -> bool {
    if ing.food_id.is_some() && !ing.needs_review {
        return true;
    }
    if ing.needs_review || ing.resolution_source.as_deref() == Some("unresolved") {
        return !retry_unresolved;
    }
    false
}

/// Minimal structured ingredient holding just a name (for preparing
/// legacy shopping rows through the same parser-first path).
const fn bare_ingredient(name: String) -> Ingredient {
    Ingredient {
        section: None,
        quantity: None,
        unit: None,
        name,
        prep: None,
        ingredient_id: None,
        raw_text: None,
        food_id: None,
        qualifiers: Vec::new(),
        resolution_source: None,
        resolution_confidence: None,
        needs_review: false,
        raw: false,
        canonical_name: None,
    }
}

/// Apply outcomes to legacy shopping rows.
async fn apply_to_shopping(
    pool: &SqlitePool,
    shopping_attempted: &[(i64, String)],
    outcomes_map: &HashMap<String, ResolutionOutcome>,
) -> anyhow::Result<u64> {
    let mut updated = 0u64;
    for (item_id, phrase) in shopping_attempted {
        let outcome = outcomes_map.get(&normalize_name(phrase));
        match outcome.and_then(|o| o.food_id) {
            Some(food_id) => {
                sqlx::query("UPDATE shopping_items SET food_id = ?, resolution_source = ? WHERE id = ?")
                    .bind(food_id)
                    .bind(outcome.and_then(|o| o.resolution_source))
                    .bind(item_id)
                    .execute(pool)
                    .await?;
                updated += 1;
            }
            None => {
                // LLM failure / genuinely unknown: mark attempted so future
                // default runs stay LLM-free.
                sqlx::query("UPDATE shopping_items SET resolution_source = 'unresolved' WHERE id = ?")
                    .bind(item_id)
                    .execute(pool)
                    .await?;
            }
        }
    }
    Ok(updated)
}

/// Run the backfill, returning a summary.
///
/// # Errors
///
/// Returns an error if the database cannot be read or written.
pub async fn run(
    pool: &SqlitePool,
    config: &Config,
    retry_unresolved: bool,
) -> anyhow::Result<BackfillStats> {
    let mut stats = BackfillStats::default();

    let mut recipes = load_recipe_ingredients(pool).await?;
    stats.recipes_scanned = recipes.len() as u64;
    stats.ingredients_scanned = recipes
        .iter()
        .map(|(_, ings)| ings.iter().filter(|i| i.section.is_none()).count() as u64)
        .sum();

    // Legacy shopping rows never touched (unresolved attempts are skipped).
    let legacy_shopping: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, name, COALESCE(resolution_source, '') FROM shopping_items \
          WHERE food_id IS NULL",
    )
    .fetch_all(pool)
    .await?;

    // Collect distinct normalized phrases across all sources that need work.
    let mut phrases: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut shopping_attempted: Vec<(i64, String)> = Vec::new();

    for (_, ings) in &mut recipes {
        for ing in ings.iter_mut().filter(|i| i.section.is_none()) {
            if is_attempted(ing, retry_unresolved) {
                stats.skipped_attempted += 1;
                continue;
            }
            let phrase = prepare_ingredient_for_resolution(ing);
            if !phrase.is_empty() {
                phrases.insert(normalize_name(&phrase));
            }
        }
    }
    for (item_id, name, source) in legacy_shopping {
        if source == "unresolved" && !retry_unresolved {
            stats.skipped_attempted += 1;
            continue;
        }
        // Legacy shopping names can still carry quantities: prepare them the
        // same way as recipe lines so the resolver only sees the phrase.
        let phrase = prepare_ingredient_for_resolution(&mut bare_ingredient(name.clone()));
        let norm = normalize_name(&phrase);
        if !norm.is_empty() {
            phrases.insert(norm);
            shopping_attempted.push((item_id, phrase));
        }
    }

    if phrases.is_empty() {
        tracing::info!("backfill: nothing to do");
        return Ok(stats);
    }

    // Resolve each distinct name once.
    let settings = LlmSettings::load(pool)
        .await
        .with_env_overrides(config.llm_model.as_deref(), config.llm_fallback_model.as_deref());
    let llm = OpenRouterFoodLlm::new(
        crate::llm::LlmClient::new(
            config.llm_api_url.clone(),
            config.llm_api_key.clone().unwrap_or_default(),
            settings.model,
        ),
        settings.fallback_model,
        config.system_prompt_food_resolver.clone(),
    );

    let phrase_list: Vec<String> = phrases.into_iter().collect();
    let outcomes = resolve_batch(pool, &llm, &phrase_list).await?;
    let outcomes_map: HashMap<String, ResolutionOutcome> =
        phrase_list.into_iter().zip(outcomes).collect();

    // Apply: recipe ingredient JSON.
    let ingredients_updated =
        apply_to_recipes(pool, recipes, &outcomes_map, retry_unresolved).await?;
    stats.ingredients_updated += ingredients_updated;

    // Apply: legacy shopping rows.
    let shopping_updated = apply_to_shopping(pool, &shopping_attempted, &outcomes_map).await?;
    stats.shopping_updated += shopping_updated;

    // Summarise based on the resolution outcomes produced.
    for outcome in outcomes_map.values() {
        match outcome.resolution_source {
            Some("llm" | "new_food") => stats.resolved_via_llm += 1,
            Some("unresolved") => {
                stats.unresolved += 1;
                stats.needs_review += 1;
            }
            _ => stats.resolved_locally += 1,
        }
        if outcome.resolution_source == Some("new_food") {
            stats.foods_created += 1;
        }
        if outcome.food_id.is_some() {
            stats.aliases_created += 1;
        }
    }

    Ok(stats)
}

/// Write resolved identity back into recipe ingredient JSON.
async fn apply_to_recipes(
    pool: &SqlitePool,
    mut recipes: Vec<(i64, Vec<Ingredient>)>,
    outcomes_map: &HashMap<String, ResolutionOutcome>,
    retry_unresolved: bool,
) -> anyhow::Result<u64> {
    let mut updated = 0u64;
    for (recipe_id, ings) in &mut recipes {
        let mut changed = false;
        for ing in ings.iter_mut() {
            if ing.section.is_some() {
                continue;
            }
            if is_attempted(ing, retry_unresolved) {
                continue;
            }
            let phrase = prepare_ingredient_for_resolution(ing);
            let Some(outcome) = outcomes_map.get(&normalize_name(&phrase)) else {
                continue;
            };
            updated += 1;
            changed = true;
            apply_outcome(ing, outcome);
        }
        if changed {
            let json = serde_json::to_string(&ings)?;
            sqlx::query("UPDATE recipes SET ingredients = json(?) WHERE id = ?")
                .bind(&json)
                .bind(*recipe_id)
                .execute(pool)
                .await?;
        }
    }
    Ok(updated)
}

/// Prepare a legacy ingredient for semantic resolution and return the
/// *ingredient phrase* the Food resolver should see.
///
/// - Raw lines and legacy rows that kept quantity/unit wording inside
///   `name` are run through the deterministic parser (conservatively: only
///   when the structured `quantity`/`unit` fields are unset, so correctly
///   structured ingredients are never touched).
/// - Price annotations like `($0.24)` are stripped before parsing so they
///   never become part of Food identity.
/// - The original line is preserved in `raw_text`.
pub fn prepare_ingredient_for_resolution(ing: &mut Ingredient) -> String {
    if ing.section.is_some() {
        return String::new();
    }

    let should_reparse = ing.raw || (ing.quantity.is_none() && ing.unit.is_none());
    if !should_reparse {
        // Already structured: `name` is the phrase.
        return ing.name.clone();
    }

    let original = ing.name.trim().to_string();
    let processed = PRICE_RE.replace_all(&original, "");
    if let Some(parsed) = parse_ingredient_line(&processed) {
        // Only adopt parser output when the line actually carried structure
        // (quantity/unit/prep); plain names stay completely untouched.
        let structured =
            parsed.quantity.is_some() || parsed.unit.is_some() || parsed.prep.is_some();
        if !structured {
            return original;
        }
        // Preserve the original line (price annotations intact).
        if ing.raw_text.is_none() {
            ing.raw_text = Some(original.clone());
        }
        if let Some(q) = parsed.quantity {
            ing.quantity = Some(q);
        }
        if let Some(u) = parsed.unit {
            ing.unit = Some(u.to_string());
        }
        ing.prep = parsed.prep;
        if !parsed.ingredient_phrase.is_empty() {
            ing.name.clone_from(&parsed.ingredient_phrase);
        }
        ing.raw = false;
        return parsed.ingredient_phrase;
    }
    ing.name.clone()
}

/// Write resolution fields onto an ingredient (never touches wording).
fn apply_outcome(ing: &mut Ingredient, outcome: &ResolutionOutcome) {
    if ing.ingredient_id.is_none() {
        ing.ingredient_id = Some(uuid::Uuid::new_v4().to_string());
    }
    if let Some(food_id) = outcome.food_id {
        ing.food_id = Some(food_id);
        ing.canonical_name.clone_from(&outcome.canonical_name);
        if !outcome.qualifiers.is_empty() {
            ing.qualifiers.clone_from(&outcome.qualifiers);
        }
        ing.resolution_source = outcome.resolution_source.map(str::to_string);
        ing.resolution_confidence = outcome.resolution_confidence;
        ing.needs_review = outcome.needs_review;
    } else if ing.food_id.is_none() {
        ing.needs_review = true;
        ing.resolution_source = Some("unresolved".to_string());
    }
}

/// Print the summary using the plan's format.
pub fn print_summary(stats: &BackfillStats) {
    println!("\nIngredient backfill\n");
    println!("Recipes scanned:         {}", stats.recipes_scanned);
    println!("Ingredients scanned:     {}", stats.ingredients_scanned);
    println!("Resolved locally:        {}", stats.resolved_locally);
    println!("Resolved via LLM:        {}", stats.resolved_via_llm);
    println!("Needs review:            {}", stats.needs_review);
    println!("Unresolved:              {}", stats.unresolved);
    println!("Foods created:           {}", stats.foods_created);
    println!("Aliases created:         {}", stats.aliases_created);
    println!("Ingredients updated:     {}", stats.ingredients_updated);
    println!("Shopping rows updated:   {}", stats.shopping_updated);
    println!("Skipped (already tried): {}", stats.skipped_attempted);
}