//! Ingredient pipeline: shared resolution pass for every save path.
//!
//! All recipes (URL/photo import, `RecipeSage`, manual, edits) funnel through
//! [`ensure_resolved`]: `raw` lines are structured with the deterministic
//! parser, stable ingredient IDs are assigned, and every non-section
//! ingredient goes through the semantic Food resolver. Recipe-visible
//! wording is never rewritten; only identity metadata is attached.

use crate::ingredients::parser::parse_ingredient_line;
use crate::ingredients::resolver::{self, OpenRouterFoodLlm};
use crate::models::{AppState, Ingredient};

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
