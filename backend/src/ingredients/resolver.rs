//! Semantic Food resolver: "what food does this phrase mean?"
//!
//! The resolver runs increasingly expensive strategies:
//!
//! A. confirmed alias hit (authoritative, never overwritten)
//! B. any exact alias hit (LLM cache)
//! C. exact canonical food name (cached as an alias afterwards)
//! D. conservative deterministic singularization (lookup only)
//! E. fuzzy candidate retrieval (strsim Jaro-Winkler) — context only
//! F. one batched LLM call per request chunk
//!
//! The LLM is an entity resolver, not a string prettifier. It can only map
//! to candidate foods it was shown, or create a new food with a validated
//! category. LLM failures never silently become "Other": unresolved phrases
//! stay `food_id = NULL` with `needs_review = true`.


use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::time::Duration;

use serde_json::Value as JsonValue;
use sqlx::SqlitePool;

use crate::ingredients::catalog;
use crate::ingredients::types::{
    CatalogSnapshot, LlmCandidate, LlmInput, LlmNewFood, LlmResolveRequest, LlmResultItem,
    ResolutionOutcome,
};
use crate::llm::LlmClient;
use crate::units::normalize_name;

/// Minimum Jaro-Winkler similarity for a Food to be offered as candidate.
const CANDIDATE_SIMILARITY_THRESHOLD: f64 = 0.75;
/// Maximum candidates offered per input phrase.
const CANDIDATE_LIMIT: usize = 5;
/// Maximum inputs per LLM call.
const LLM_CHUNK_SIZE: usize = 30;

/* =========================
 * LLM backend
 * ========================= */

/// Semantic ingredient resolution backend (LLM in production, mock in tests).
pub trait FoodLlm: Send + Sync {
    /// Resolve one batch of inputs. Implementations must validate their
    /// model output against `req` before returning (candidate food ids,
    /// category ids, mutual exclusion of `food_id`/`new_food`).
    ///
    /// # Errors
    ///
    /// Returns an error when the backend call fails; the resolver treats any
    /// error as "leave this batch unresolved", never as a fallback category.
    fn resolve(
        &self,
        req: &LlmResolveRequest,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<LlmResultItem>>> + Send;
}

/// OpenRouter-backed production resolver.
pub struct OpenRouterFoodLlm {
    llm: LlmClient,
    fallback_model: String,
    system_prompt: String,
}

impl OpenRouterFoodLlm {
    /// Build from an [`LlmClient`], a fallback model, and the resolver
    /// system prompt (`BLAZ_SYSTEM_PROMPT_FOOD_RESOLVER`).
    #[must_use]
    pub const fn new(llm: LlmClient, fallback_model: String, system_prompt: String) -> Self {
        Self {
            llm,
            fallback_model,
            system_prompt,
        }
    }

    /// Build from app state (models from DB settings, prompt from config).
    /// Without an API key the resolver still serves the deterministic
    /// strategies and fails fast on LLM batches.
    pub async fn from_state(state: &crate::models::AppState) -> Self {
        use crate::routes::settings::LlmSettings;
        let settings = LlmSettings::load(&state.pool)
            .await
            .with_env_overrides(
                state.config.llm_model.as_deref(),
                state.config.llm_fallback_model.as_deref(),
            );
        Self::new(
            LlmClient::new(
                state.config.llm_api_url.clone(),
                state.config.llm_api_key.clone().unwrap_or_default(),
                settings.model,
            ),
            settings.fallback_model,
            state.config.system_prompt_food_resolver.clone(),
        )
    }
}

impl FoodLlm for OpenRouterFoodLlm {
    async fn resolve(&self, req: &LlmResolveRequest) -> anyhow::Result<Vec<LlmResultItem>> {
        if self.llm.token.trim().is_empty() {
            anyhow::bail!("LLM API key is not configured");
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(45))
            .build()?;
        let user = build_user_prompt(req);
        let value = self
            .llm
            .chat_json_with_fallback(
                &http,
                &self.fallback_model,
                &self.system_prompt,
                &user,
                0.1,
                Duration::from_secs(45),
                Some(4000),
            )
            .await?;
        Ok(parse_llm_response(&value, req))
    }
}

/* =========================
 * Prompt building / response validation
 * ========================= */

fn build_user_prompt(req: &LlmResolveRequest) -> String {
    let mut out = String::new();
    out.push_str("Valid categories (id = name):\n");
    for (id, name) in &req.categories {
        let _ = writeln!(out, "{id} = {name}");
    }
    out.push_str("\nIngredients to resolve:\n");
    for (index, input) in req.inputs.iter().enumerate() {
        let _ = writeln!(out, "\n{index}. \"{}\"", input.phrase);
        if input.candidates.is_empty() {
            out.push_str("   Candidates: none\n");
        } else {
            let list = input
                .candidates
                .iter()
                .map(|c| {
                    c.matched_via.as_ref().map_or_else(
                        || format!("#{} {}", c.food_id, c.name),
                        |alias| format!("#{} {} (known as \"{alias}\")", c.food_id, c.name),
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(out, "   Candidates: {list}");
        }
    }
    out.push_str("\nReturn {\"results\":[...]} with one entry per input index, in order.\n");
    out
}

/// Validate an LLM JSON response against the request it answers.
///
/// Food ids must be among that input's candidates; category ids must be in
/// the valid list (invalid ones are dropped, not guessed); `food_id` and
/// `new_food` are mutually exclusive; out-of-range entries are ignored.
fn parse_llm_response(value: &JsonValue, req: &LlmResolveRequest) -> Vec<LlmResultItem> {
    let Some(entries) = value.get("results").and_then(JsonValue::as_array) else {
        tracing::warn!("resolver LLM response is missing the 'results' array");
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in entries {
        let Some(raw_index) = entry.get("input_index").and_then(JsonValue::as_u64) else {
            tracing::warn!("resolver LLM entry without input_index; ignoring");
            continue;
        };
        let Some(index) = usize::try_from(raw_index)
            .ok()
            .filter(|&i| i < req.inputs.len())
        else {
            tracing::warn!(
                input_index = raw_index,
                "resolver LLM entry index out of range; ignoring"
            );
            continue;
        };
        let input = &req.inputs[index];

        let qualifiers: Vec<String> = entry
            .get("qualifiers")
            .and_then(JsonValue::as_array)
            .map_or_else(Vec::new, |arr| {
                arr.iter()
                    .filter_map(|q| q.as_str().map(str::to_string))
                    .collect()
            });
        let needs_review = entry
            .get("needs_review")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);

        let food_id = match entry.get("food_id").and_then(JsonValue::as_i64) {
            Some(id) if input.candidates.iter().any(|c| c.food_id == id) => Some(id),
            Some(id) => {
                tracing::warn!(
                    food_id = id,
                    input_index = index,
                    "resolver LLM returned a food_id outside the candidate list; ignoring"
                );
                None
            }
            None => None,
        };

        let new_food = entry
            .get("new_food")
            .filter(|v| !v.is_null())
            .and_then(|v| {
                let name = v
                    .get("canonical_name")
                    .and_then(JsonValue::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())?;
                let category_id = match v.get("category_id").and_then(JsonValue::as_i64) {
                    Some(cid) if req.categories.iter().any(|(id, _)| *id == cid) => Some(cid),
                    Some(cid) => {
                        tracing::warn!(
                            category_id = cid,
                            "resolver invalid category ID; new food starts uncategorized"
                        );
                        None
                    }
                    None => None,
                };
                Some(LlmNewFood {
                    canonical_name: name.to_string(),
                    category_id,
                })
            });

        let (food_id, new_food, needs_review) = if food_id.is_some() && new_food.is_some() {
            tracing::warn!(
                input_index = index,
                "resolver LLM returned both food_id and new_food; marking for review"
            );
            (None, None, true)
        } else {
            (food_id, new_food, needs_review)
        };

        out.push(LlmResultItem {
            input_index: index,
            food_id,
            new_food,
            qualifiers,
            needs_review,
        });
    }
    out
}

/* =========================
 * Deterministic strategies (A–D)
 * ========================= */

fn singularize_last_word(word: &str) -> Option<String> {
    if word.len() <= 3 {
        return None;
    }
    if let Some(stem) = word.strip_suffix("ies") {
        return Some(format!("{stem}y"));
    }
    if let Some(stem) = word.strip_suffix("es") {
        return Some(stem.to_string());
    }
    if let Some(stem) = word.strip_suffix('s')
        && !word.ends_with("ss")
    {
        return Some(stem.to_string());
    }
    None
}

/// Singularize the final word of a normalized phrase (`ies` → `y`,
/// `es` → stripped, trailing `s` → stripped). Conservative by design:
/// the result is only ever used as a *lookup* against existing foods and
/// aliases, never as an identity claim.
fn singularized_phrase(normalized: &str) -> Option<String> {
    let words: Vec<&str> = normalized.split_whitespace().collect();
    let last = *words.last()?;
    let singular = singularize_last_word(last)?;
    if words.len() == 1 {
        return Some(singular);
    }
    Some(format!(
        "{} {singular}",
        words[..words.len() - 1].join(" ")
    ))
}

/// Strategies A–D: local, deterministic lookups.
///
/// Returns `Ok(None)` when the phrase must go to the LLM.
async fn resolve_deterministic(
    pool: &SqlitePool,
    phrase: &str,
) -> anyhow::Result<Option<ResolutionOutcome>> {
    let normalized = normalize_name(phrase);
    if normalized.is_empty() {
        return Ok(Some(ResolutionOutcome::unresolved()));
    }

    // A/B: exact alias (confirmed mappings are authoritative).
    if let Some(alias) = catalog::find_alias(pool, phrase).await?
        && let Some(food) = catalog::get_food_by_id(pool, alias.food_id).await?
    {
        let confidence = if alias.confirmed {
            Some(1.0)
        } else {
            alias.confidence
        };
        tracing::info!(
            phrase = %phrase,
            food_id = food.id,
            confirmed = alias.confirmed,
            "ingredient_resolution_alias_hit"
        );
        return Ok(Some(ResolutionOutcome {
            food_id: Some(food.id),
            canonical_name: Some(food.canonical_name),
            qualifiers: Vec::new(),
            resolution_source: Some(if alias.confirmed {
                "confirmed_alias"
            } else {
                "alias"
            }),
            resolution_confidence: confidence,
            needs_review: false,
        }));
    }

    // C: exact canonical food name; cache an alias so B short-circuits next time.
    if let Some(food) = catalog::get_food_by_name(pool, phrase).await? {
        catalog::create_alias(pool, phrase, food.id, "automatic", false, Some(1.0)).await?;
        tracing::info!(phrase = %phrase, food_id = food.id, "ingredient_resolution_food_hit");
        return Ok(Some(ResolutionOutcome {
            food_id: Some(food.id),
            canonical_name: Some(food.canonical_name),
            qualifiers: Vec::new(),
            resolution_source: Some("food"),
            resolution_confidence: Some(1.0),
            needs_review: false,
        }));
    }

    // D: conservative deterministic singularization (lookup only, never creation).
    if let Some(singular) = singularized_phrase(&normalized) {
        let hit = match catalog::get_food_by_name(pool, &singular).await? {
            Some(food) => Some(food),
            None => match catalog::find_alias(pool, &singular).await? {
                Some(alias) => catalog::get_food_by_id(pool, alias.food_id).await?,
                None => None,
            },
        };
        if let Some(food) = hit {
            catalog::create_alias(pool, phrase, food.id, "automatic", false, Some(0.9)).await?;
            tracing::info!(
                phrase = %phrase,
                food_id = food.id,
                "ingredient_resolution_deterministic_hit"
            );
            return Ok(Some(ResolutionOutcome {
                food_id: Some(food.id),
                canonical_name: Some(food.canonical_name),
                qualifiers: Vec::new(),
                resolution_source: Some("deterministic"),
                resolution_confidence: Some(0.9),
                needs_review: false,
            }));
        }
    }

    Ok(None)
}

/* =========================
 * Candidate retrieval (E)
 * ========================= */

/// Candidate score for one catalog reference against a phrase.
///
/// Food names or aliases that appear verbatim (as token n-grams, optionally
/// singularized) inside the phrase score highest; otherwise Jaro-Winkler
/// similarity must reach the threshold. `None` means "not a candidate".
fn candidate_score(
    ngrams: &HashSet<String>,
    phrase: &str,
    reference: &str,
) -> Option<f64> {
    if ngrams.contains(reference) {
        return Some(1.0);
    }
    let sim = strsim::jaro_winkler(phrase, reference);
    (sim >= CANDIDATE_SIMILARITY_THRESHOLD).then_some(sim)
}

/// Fuzzy-match a phrase against the catalog; candidate generation only,
/// never an identity claim. Token n-grams (up to 3 words, singularized
/// variants included) match verbatim; everything else is ranked by
/// Jaro-Winkler similarity, then name, deduplicated per food.
fn candidate_matches(
    snapshot: &CatalogSnapshot,
    normalized: &str,
    limit: usize,
) -> Vec<LlmCandidate> {
    let phrase_words: Vec<&str> = normalized.split_whitespace().collect();
    let mut ngrams: HashSet<String> = HashSet::new();
    let max_size = phrase_words.len().min(3);
    for size in 1..=max_size {
        for window in phrase_words.windows(size) {
            let joined = window.join(" ");
            if let Some(singular) = singularized_phrase(&joined) {
                ngrams.insert(singular);
            }
            ngrams.insert(joined);
        }
    }

    let names: HashMap<i64, &str> = snapshot
        .foods
        .iter()
        .map(|f| (f.id, f.canonical_name.as_str()))
        .collect();

    let mut scored: Vec<(f64, LlmCandidate)> = Vec::new();
    for food in &snapshot.foods {
        if let Some(score) = candidate_score(&ngrams, normalized, &food.normalized_name) {
            scored.push((
                score,
                LlmCandidate {
                    food_id: food.id,
                    name: food.canonical_name.clone(),
                    matched_via: None,
                },
            ));
        }
    }
    for alias in &snapshot.aliases {
        let Some(name) = names.get(&alias.food_id) else {
            continue;
        };
        if let Some(score) = candidate_score(&ngrams, normalized, &alias.normalized_alias) {
            scored.push((
                score,
                LlmCandidate {
                    food_id: alias.food_id,
                    name: (*name).to_string(),
                    matched_via: Some(alias.alias.clone()),
                },
            ));
        }
    }

    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));

    let mut seen: HashSet<i64> = HashSet::new();
    let mut out = Vec::new();
    for (_, candidate) in scored {
        if seen.insert(candidate.food_id) {
            out.push(candidate);
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

/* =========================
 * Batch resolution
 * ========================= */

/// Resolve ingredient phrases to canonical Foods.
///
/// Returns one outcome per input phrase, in order. Strategies A–D are
/// local and deterministic; remaining phrases go through one batched LLM
/// call per [`LLM_CHUNK_SIZE`] inputs. LLM failures leave phrases
/// unresolved with `needs_review = true` — no fabricated foods, no
/// fallback categories.
///
/// # Errors
///
/// Returns an error if a database read/write fails. LLM errors are
/// contained per batch and logged.
pub async fn resolve_batch<R: FoodLlm>(
    pool: &SqlitePool,
    llm: &R,
    phrases: &[String],
) -> anyhow::Result<Vec<ResolutionOutcome>> {
    let mut outcomes: Vec<Option<ResolutionOutcome>> = vec![None; phrases.len()];
    let mut representatives: Vec<usize> = Vec::new();
    let mut rep_index: HashMap<String, usize> = HashMap::new();

    for (i, phrase) in phrases.iter().enumerate() {
        match resolve_deterministic(pool, phrase).await? {
            Some(outcome) => outcomes[i] = Some(outcome),
            None => {
                // Deduplicate identical phrases so each is resolved once.
                if let Entry::Vacant(entry) = rep_index.entry(normalize_name(phrase)) {
                    entry.insert(i);
                    representatives.push(i);
                }
            }
        }
    }

    if !representatives.is_empty() {
        let snapshot = catalog::load_catalog_snapshot(pool).await?;
        let categories = catalog::list_categories(pool).await?;

        for chunk in representatives.chunks(LLM_CHUNK_SIZE) {
            let inputs: Vec<LlmInput> = chunk
                .iter()
                .map(|&i| LlmInput {
                    phrase: phrases[i].clone(),
                    candidates: candidate_matches(
                        &snapshot,
                        &normalize_name(&phrases[i]),
                        CANDIDATE_LIMIT,
                    ),
                })
                .collect();
            let request = LlmResolveRequest {
                inputs,
                categories: categories.clone(),
            };

            let results = match llm.resolve(&request).await {
                Ok(results) => results,
                Err(e) => {
                    tracing::warn!(
                        ?e,
                        batch_size = chunk.len(),
                        "resolver LLM failure; leaving phrases unresolved"
                    );
                    continue;
                }
            };
            tracing::info!(batch_size = chunk.len(), "ingredient_resolution_llm");

            apply_llm_results(pool, &categories, phrases, chunk, results, &mut outcomes).await?;
        }
    }

    // Duplicate phrases share their representative's outcome.
    for (i, phrase) in phrases.iter().enumerate() {
        if outcomes[i].is_none() {
            let normalized = normalize_name(phrase);
            if let Some(&rep) = rep_index.get(&normalized)
                && rep != i
                && let Some(outcome) = outcomes[rep].as_ref()
            {
                outcomes[i] = Some(outcome.clone());
            }
        }
    }

    Ok(outcomes
        .into_iter()
        .map(|o| o.unwrap_or_else(ResolutionOutcome::unresolved))
        .collect())
}

/// Persist and record one batch of validated LLM decisions.
async fn apply_llm_results(
    pool: &SqlitePool,
    categories: &[(i64, String)],
    phrases: &[String],
    chunk: &[usize],
    results: Vec<LlmResultItem>,
    outcomes: &mut [Option<ResolutionOutcome>],
) -> anyhow::Result<()> {
    for result in results {
        let Some(&i) = chunk.get(result.input_index) else {
            continue;
        };

        if let Some(food_id) = result.food_id {
            let Some(food) = catalog::get_food_by_id(pool, food_id).await? else {
                continue;
            };
            catalog::create_alias(pool, &phrases[i], food.id, "automatic", false, None).await?;
            outcomes[i] = Some(ResolutionOutcome {
                food_id: Some(food.id),
                canonical_name: Some(food.canonical_name),
                qualifiers: result.qualifiers,
                resolution_source: Some("llm"),
                resolution_confidence: None,
                needs_review: result.needs_review,
            });
        } else if let Some(new_food) = result.new_food {
            let category_id = new_food
                .category_id
                .filter(|cid| categories.iter().any(|(id, _)| id == cid));
            let food =
                catalog::create_food(pool, &new_food.canonical_name, category_id, "llm", None)
                    .await?;
            catalog::create_alias(pool, &phrases[i], food.id, "automatic", false, None).await?;
            tracing::info!(
                canonical = %food.canonical_name,
                category_id = ?category_id,
                "ingredient_resolution_new_food"
            );
            if category_id.is_some() {
                tracing::info!(
                    food_id = food.id,
                    category_id = ?category_id,
                    "food_category_llm"
                );
            }
            outcomes[i] = Some(ResolutionOutcome {
                food_id: Some(food.id),
                canonical_name: Some(food.canonical_name),
                qualifiers: result.qualifiers,
                resolution_source: Some("new_food"),
                resolution_confidence: None,
                needs_review: result.needs_review,
            });
        } else {
            tracing::info!(phrase = %phrases[i], "ingredient_resolution_needs_review");
            outcomes[i] = Some(ResolutionOutcome::unresolved());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    async fn test_pool() -> SqlitePool {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        crate::db::MIGRATOR.run(&pool).await.expect("migrations");
        pool
    }

    async fn category_id(pool: &SqlitePool, name: &str) -> i64 {
        sqlx::query_as::<_, (i64,)>("SELECT id FROM shopping_categories WHERE name = ?")
            .bind(name)
            .fetch_one(pool)
            .await
            .expect("category exists")
            .0
    }

    fn phrases(list: &[&str]) -> Vec<String> {
        list.iter().map(std::string::ToString::to_string).collect()
    }

    enum MockPlan {
        Fail,
        MapFirstCandidate,
        NewFood {
            name: &'static str,
            category: Option<i64>,
        },
    }

    struct MockFoodLlm {
        plan: MockPlan,
        calls: AtomicUsize,
    }

    impl MockFoodLlm {
        fn new(plan: MockPlan) -> Self {
            Self {
                plan,
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl FoodLlm for MockFoodLlm {
        async fn resolve(&self, req: &LlmResolveRequest) -> anyhow::Result<Vec<LlmResultItem>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match &self.plan {
                MockPlan::Fail => anyhow::bail!("mock LLM outage"),
                MockPlan::MapFirstCandidate => Ok(req
                    .inputs
                    .iter()
                    .enumerate()
                    .map(|(i, input)| {
                        input.candidates.first().map_or_else(
                            || LlmResultItem {
                                input_index: i,
                                food_id: None,
                                new_food: None,
                                qualifiers: Vec::new(),
                                needs_review: true,
                            },
                            |c| LlmResultItem {
                                input_index: i,
                                food_id: Some(c.food_id),
                                new_food: None,
                                qualifiers: vec!["large".to_string()],
                                needs_review: false,
                            },
                        )
                    })
                    .collect()),
                MockPlan::NewFood { name, category } => Ok(req
                    .inputs
                    .iter()
                    .enumerate()
                    .map(|(i, _)| LlmResultItem {
                        input_index: i,
                        food_id: None,
                        new_food: Some(LlmNewFood {
                            canonical_name: (*name).to_string(),
                            category_id: *category,
                        }),
                        qualifiers: Vec::new(),
                        needs_review: false,
                    })
                    .collect()),
            }
        }
    }

    /* ---------- deterministic strategies skip the LLM ---------- */

    #[tokio::test]
    async fn alias_hits_skip_llm() {
        let pool = test_pool().await;
        let potato = catalog::create_food(&pool, "potato", None, "unknown", None)
            .await
            .expect("potato food");
        catalog::create_alias(&pool, "potatoes", potato.id, "automatic", false, None)
            .await
            .expect("unconfirmed alias");
        catalog::confirm_alias(&pool, "spuds", potato.id)
            .await
            .expect("confirmed alias");

        let llm = MockFoodLlm::new(MockPlan::Fail);
        let out = resolve_batch(&pool, &llm, &phrases(&["potatoes", "SPUDS"]))
            .await
            .expect("resolve");
        assert_eq!(llm.calls(), 0);
        assert_eq!(out[0].food_id, Some(potato.id));
        assert_eq!(out[0].resolution_source, Some("alias"));
        assert_eq!(out[1].resolution_source, Some("confirmed_alias"));
        assert!(!out[0].needs_review);
    }

    #[tokio::test]
    async fn exact_food_hit_skips_llm_and_caches_alias() {
        let pool = test_pool().await;
        let onion = catalog::create_food(&pool, "onion", None, "unknown", None)
            .await
            .expect("onion food");

        let llm = MockFoodLlm::new(MockPlan::Fail);
        let out = resolve_batch(&pool, &llm, &phrases(&["onion"]))
            .await
            .expect("resolve");
        assert_eq!(out[0].food_id, Some(onion.id));
        assert_eq!(out[0].resolution_source, Some("food"));
        assert_eq!(llm.calls(), 0);

        // Second run hits the alias cached by the first.
        let out = resolve_batch(&pool, &llm, &phrases(&["Onion"]))
            .await
            .expect("resolve again");
        assert_eq!(out[0].resolution_source, Some("alias"));
        assert_eq!(llm.calls(), 0);
    }

    #[tokio::test]
    async fn deterministic_singularization_resolves_plurals() {
        let pool = test_pool().await;
        let potato = catalog::create_food(&pool, "potato", None, "unknown", None)
            .await
            .expect("potato food");

        let llm = MockFoodLlm::new(MockPlan::Fail);
        let out = resolve_batch(&pool, &llm, &phrases(&["potatoes"]))
            .await
            .expect("resolve");
        assert_eq!(out[0].food_id, Some(potato.id));
        assert_eq!(out[0].resolution_source, Some("deterministic"));
        assert_eq!(llm.calls(), 0);

        // The plural alias is cached, so the next run is an alias hit.
        let out = resolve_batch(&pool, &llm, &phrases(&["POTATOES "]))
            .await
            .expect("resolve again");
        assert_eq!(out[0].resolution_source, Some("alias"));
        assert_eq!(llm.calls(), 0);
    }

    /* ---------- semantic guards ---------- */

    #[tokio::test]
    async fn compound_foods_are_never_auto_merged() {
        let pool = test_pool().await;
        for name in ["potato", "milk", "sugar", "butter", "onion"] {
            catalog::create_food(&pool, name, None, "unknown", None)
                .await
                .expect("base food");
        }

        // No LLM available: none of these may resolve deterministically
        // or through fuzzy matching.
        let llm = MockFoodLlm::new(MockPlan::Fail);
        let inputs = [
            "sweet potato",
            "coconut milk",
            "brown sugar",
            "peanut butter",
            "potato starch",
            "onion powder",
        ];
        let out = resolve_batch(&pool, &llm, &phrases(&inputs))
            .await
            .expect("resolve");
        for (i, phrase) in inputs.iter().enumerate() {
            assert_eq!(out[i].food_id, None, "'{phrase}' must not auto-merge");
            assert!(out[i].needs_review);
        }

        // Fuzzy retrieval only generates candidates; it never merges.
        let snapshot = catalog::load_catalog_snapshot(&pool).await.expect("snapshot");
        let candidates = candidate_matches(&snapshot, "large potatoes", CANDIDATE_LIMIT);
        assert!(
            candidates.iter().any(|c| c.name == "potato"),
            "lookalike phrases generate candidates"
        );
    }

    /* ---------- LLM path ---------- */

    #[tokio::test]
    async fn llm_maps_to_candidate_and_caches() {
        let pool = test_pool().await;
        let potato = catalog::create_food(&pool, "potato", None, "unknown", None)
            .await
            .expect("potato food");

        let llm = MockFoodLlm::new(MockPlan::MapFirstCandidate);
        let out = resolve_batch(&pool, &llm, &phrases(&["large potatoes"]))
            .await
            .expect("resolve");
        assert_eq!(llm.calls(), 1);
        assert_eq!(out[0].food_id, Some(potato.id));
        assert_eq!(out[0].resolution_source, Some("llm"));
        assert_eq!(out[0].qualifiers, vec!["large".to_string()]);
        assert!(!out[0].needs_review);

        // Now cached; no further LLM calls.
        let out = resolve_batch(&pool, &llm, &phrases(&["large potatoes"]))
            .await
            .expect("resolve again");
        assert_eq!(llm.calls(), 1);
        assert_eq!(out[0].resolution_source, Some("alias"));
    }

    #[tokio::test]
    async fn llm_creates_new_food_once() {
        let pool = test_pool().await;
        let vegetables = category_id(&pool, "Vegetables").await;

        let llm = MockFoodLlm::new(MockPlan::NewFood {
            name: "gochujang",
            category: Some(vegetables),
        });
        let out = resolve_batch(&pool, &llm, &phrases(&["gochujang"]))
            .await
            .expect("resolve");
        assert_eq!(llm.calls(), 1);
        let food_id = out[0].food_id.expect("resolved to a new food");
        assert_eq!(out[0].canonical_name.as_deref(), Some("gochujang"));
        assert_eq!(out[0].resolution_source, Some("new_food"));

        let food = catalog::get_food_by_id(&pool, food_id)
            .await
            .expect("reload")
            .expect("food exists");
        assert_eq!(food.category_id, Some(vegetables));
        assert_eq!(food.category_source, "llm");

        // Second resolve: alias hit, no new food, no LLM call.
        let out = resolve_batch(&pool, &llm, &phrases(&["gochujang"]))
            .await
            .expect("resolve again");
        assert_eq!(llm.calls(), 1);
        assert_eq!(out[0].food_id, Some(food_id));
        assert_eq!(out[0].resolution_source, Some("alias"));

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM foods")
                .fetch_one(&pool)
                .await
                .expect("count foods");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn llm_failure_leaves_unresolved_without_junk_foods() {
        let pool = test_pool().await;
        catalog::create_food(&pool, "potato", None, "unknown", None)
            .await
            .expect("potato food");

        let llm = MockFoodLlm::new(MockPlan::Fail);
        let out = resolve_batch(&pool, &llm, &phrases(&["gochujang", "potato"]))
            .await
            .expect("resolve");
        assert_eq!(llm.calls(), 1);

        // Known food still resolves during the outage.
        assert!(out[1].food_id.is_some());
        // Unknown phrase: unresolved, needs review, never a fabricated food.
        assert_eq!(out[0].food_id, None);
        assert!(out[0].needs_review);
        assert_eq!(out[0].resolution_source, Some("unresolved"));

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM foods")
                .fetch_one(&pool)
                .await
                .expect("count foods");
        assert_eq!(count, 1, "no junk food created from the failed phrase");
    }

    #[tokio::test]
    async fn duplicate_phrases_share_one_llm_call() {
        let pool = test_pool().await;
        let llm = MockFoodLlm::new(MockPlan::NewFood {
            name: "ajvar",
            category: None,
        });

        let out = resolve_batch(&pool, &llm, &phrases(&["ajvar", "Ajvar ", "AJVAR"]))
            .await
            .expect("resolve");
        assert_eq!(llm.calls(), 1);
        assert!(out[0].food_id.is_some());
        assert_eq!(out[0].food_id, out[1].food_id);
        assert_eq!(out[1].food_id, out[2].food_id);
    }

    #[tokio::test]
    async fn large_batches_are_chunked_per_llm_call() {
        let pool = test_pool().await;
        let llm = MockFoodLlm::new(MockPlan::NewFood {
            name: "misc",
            category: None,
        });

        let list: Vec<String> = (0..LLM_CHUNK_SIZE + 5)
            .map(|i| format!("item {i}"))
            .collect();
        let out = resolve_batch(&pool, &llm, &list).await.expect("resolve");
        assert_eq!(llm.calls(), 2);
        assert!(out.iter().all(|o| o.food_id.is_some()));
    }

    #[tokio::test]
    async fn empty_phrases_are_flagged_for_review() {
        let pool = test_pool().await;
        let llm = MockFoodLlm::new(MockPlan::Fail);
        let out = resolve_batch(&pool, &llm, &phrases(&["", "   "]))
            .await
            .expect("resolve");
        assert_eq!(llm.calls(), 0);
        assert!(
            out.iter()
                .all(|o| o.food_id.is_none() && o.needs_review)
        );
    }

    /* ---------- LLM response validation (§9F) ---------- */

    fn sample_request() -> LlmResolveRequest {
        LlmResolveRequest {
            inputs: vec![LlmInput {
                phrase: "large potatoes".to_string(),
                candidates: vec![LlmCandidate {
                    food_id: 42,
                    name: "potato".to_string(),
                    matched_via: None,
                }],
            }],
            categories: vec![(2, "Vegetables".to_string()), (1, "Other".to_string())],
        }
    }

    #[test]
    fn parse_llm_response_accepts_valid_mapping() {
        let req = sample_request();
        let value = serde_json::json!({
            "results": [
                {"input_index": 0, "food_id": 42, "new_food": null,
                 "qualifiers": ["large"], "needs_review": false}
            ]
        });
        let out = parse_llm_response(&value, &req);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].food_id, Some(42));
        assert_eq!(out[0].qualifiers, vec!["large".to_string()]);
        assert!(!out[0].needs_review);
    }

    #[test]
    fn parse_llm_response_rejects_invented_ids() {
        let req = sample_request();

        // food_id outside the candidate list → dropped.
        let value = serde_json::json!({
            "results": [
                {"input_index": 0, "food_id": 999, "new_food": null,
                 "qualifiers": [], "needs_review": false}
            ]
        });
        let out = parse_llm_response(&value, &req);
        assert_eq!(out[0].food_id, None);

        // Invalid category → the new food is kept but starts uncategorized.
        let value = serde_json::json!({
            "results": [
                {"input_index": 0, "food_id": null,
                 "new_food": {"canonical_name": "gochujang", "category_id": 77},
                 "qualifiers": [], "needs_review": false}
            ]
        });
        let out = parse_llm_response(&value, &req);
        let new_food = out[0].new_food.as_ref().expect("new food kept");
        assert_eq!(new_food.canonical_name, "gochujang");
        assert_eq!(new_food.category_id, None);

        // Both food_id and new_food → neither kept, marked for review.
        let value = serde_json::json!({
            "results": [
                {"input_index": 0, "food_id": 42,
                 "new_food": {"canonical_name": "x", "category_id": null},
                 "qualifiers": [], "needs_review": false}
            ]
        });
        let out = parse_llm_response(&value, &req);
        assert_eq!(out[0].food_id, None);
        assert!(out[0].new_food.is_none());
        assert!(out[0].needs_review);

        // Out-of-range index and missing results are ignored.
        let value = serde_json::json!({
            "results": [
                {"input_index": 5, "food_id": 42, "new_food": null,
                 "qualifiers": [], "needs_review": false}
            ]
        });
        assert!(parse_llm_response(&value, &req).is_empty());
        assert!(parse_llm_response(&serde_json::json!({}), &req).is_empty());
    }

    /* ---------- helpers ---------- */

    #[test]
    fn singularization_rules() {
        assert_eq!(singularized_phrase("potatoes").as_deref(), Some("potato"));
        assert_eq!(singularized_phrase("onions").as_deref(), Some("onion"));
        assert_eq!(singularized_phrase("berries").as_deref(), Some("berry"));
        assert_eq!(
            singularized_phrase("large potatoes").as_deref(),
            Some("large potato")
        );
        assert_eq!(singularized_phrase("milk"), None);
        assert_eq!(singularized_phrase("class"), None);
        assert_eq!(singularized_phrase("kiss"), None);
    }
}
