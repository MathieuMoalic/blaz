//! Idempotent ingredient backfill.
//!
//! `blaz backfill-ingredients` resolves legacy recipes and shopping rows to
//! stable Food identity: distinct ingredient names are resolved once each
//! (deterministically first, via the batched LLM afterwards), then
//! `food_id` is written back to the shopping rows and recipe JSON.
//!
//! Safe to run repeatedly: the resolver caches aliases, so a second run
//! resolves everything locally and creates nothing.

use std::collections::HashMap;

use sqlx::SqlitePool;

use crate::config::Config;
use crate::ingredients::parser::parse_ingredient_line;
use crate::ingredients::resolver::{OpenRouterFoodLlm, resolve_batch};
use crate::ingredients::types::ResolutionOutcome;
use crate::models::Ingredient;
use crate::routes::settings::LlmSettings;
use crate::units::normalize_name;

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

/// Run the backfill, returning a summary.
///
/// # Errors
///
/// Returns an error if the database cannot be read or written.
pub async fn run(pool: &SqlitePool, config: &Config) -> anyhow::Result<BackfillStats> {
    let mut stats = BackfillStats::default();

    let recipes = load_recipe_ingredients(pool).await?;
    stats.recipes_scanned = recipes.len() as u64;
    stats.ingredients_scanned = recipes
        .iter()
        .map(|(_, ings)| ings.iter().filter(|i| i.section.is_none()).count() as u64)
        .sum();

    let legacy_shopping: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, name FROM shopping_items WHERE food_id IS NULL")
            .fetch_all(pool)
            .await?;

    // Collect distinct normalized phrases across all sources.
    let mut phrases: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for (_, ings) in &recipes {
        for ing in ings.iter().filter(|i| i.section.is_none()) {
            let phrase = ingredient_phrase(ing);
            if !phrase.is_empty() {
                phrases.insert(normalize_name(&phrase));
            }
        }
    }
    for (_, name) in &legacy_shopping {
        let norm = normalize_name(name);
        if !norm.is_empty() {
            phrases.insert(norm);
        }
    }

    if phrases.is_empty() {
        tracing::info!("backfill: nothing to do");
        return Ok(stats);
    }

    // Resolve each distinct name once.
    let settings = LlmSettings::load(pool).await;
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
    let ingredients_updated = apply_to_recipes(pool, recipes, &outcomes_map).await?;
    stats.ingredients_updated += ingredients_updated;

    for (item_id, name) in legacy_shopping {
        if let Some(outcome) = outcomes_map.get(&normalize_name(&name))
            && let Some(food_id) = outcome.food_id
        {
            sqlx::query("UPDATE shopping_items SET food_id = ? WHERE id = ?")
                .bind(food_id)
                .bind(item_id)
                .execute(pool)
                .await?;
            stats.shopping_updated += 1;
        }
    }

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
    recipes: Vec<(i64, Vec<Ingredient>)>,
    outcomes_map: &HashMap<String, ResolutionOutcome>,
) -> anyhow::Result<u64> {
    let mut updated = 0u64;
    for (recipe_id, mut ings) in recipes {
        let mut changed = false;
        for ing in &mut ings {
            if ing.section.is_some() {
                continue;
            }
            let phrase = ingredient_phrase(ing);
            let Some(outcome) = outcomes_map.get(&normalize_name(&phrase)) else {
                continue;
            };
            // Skip ingredients already carrying this identity (idempotency).
            if ing.food_id == outcome.food_id
                && ing.resolution_source.as_deref() == outcome.resolution_source
            {
                continue;
            }
            updated += 1;
            changed = true;
            apply_outcome(ing, outcome);
        }
        if changed {
            let json = serde_json::to_string(&ings)?;
            sqlx::query("UPDATE recipes SET ingredients = json(?) WHERE id = ?")
                .bind(&json)
                .bind(recipe_id)
                .execute(pool)
                .await?;
        }
    }
    Ok(updated)
}

/// The phrase to resolve for a recipe ingredient (raw lines are parsed).
fn ingredient_phrase(ing: &Ingredient) -> String {
    if ing.raw {
        parse_ingredient_line(&ing.name)
            .map_or_else(|| ing.name.clone(), |p| p.ingredient_phrase)
    } else {
        ing.name.clone()
    }
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
}