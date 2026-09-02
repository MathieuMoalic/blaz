//! Semantic regression fixtures from the production-catalog audit.
//!
//! These lock in the identity rules that the first backfill violated:
//! compound foods stay distinct, automatic aliases never shadow another
//! Food's canonical name, canonical names outrank stale aliases, and batch
//! LLM responses can only address their own work item.

use sqlx::SqlitePool;

use super::catalog::{self, AliasConflict, check_alias_identity};
use super::resolver::test_support::{MockFoodLlm, MockPlan};
use super::resolver::{self, CANDIDATE_LIMIT, FoodLlm, resolve_batch};
use super::types::{
    CatalogFoodRef, CatalogSnapshot, LlmCandidate, LlmInput, LlmNewFood, LlmResolveRequest,
    LlmResultItem,
};

async fn test_pool() -> SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    crate::db::MIGRATOR.run(&pool).await.expect("migrations");
    pool
}

async fn food_id(pool: &SqlitePool, name: &str) -> i64 {
    catalog::get_food_by_name(pool, name)
        .await
        .expect("lookup")
        .unwrap_or_else(|| panic!("food {name} missing"))
        .id
}

fn phrases(list: &[&str]) -> Vec<String> {
    list.iter().map(std::string::ToString::to_string).collect()
}

/* =========================
 * Canonical name precedence
 * ========================= */

/// Regression: the audit DB contains `coconut milk -> milk` (automatic).
/// A canonical Food `coconut milk` must win over any stale alias.
#[tokio::test]
async fn canonical_name_outranks_stale_alias() {
    let pool = test_pool().await;
    let milk = catalog::create_food(&pool, "milk", None, "unknown", None)
        .await
        .expect("milk");
    let coconut_milk = catalog::create_food(&pool, "coconut milk", None, "unknown", None)
        .await
        .expect("coconut milk");

    // Simulate the legacy corrupt mapping.
    sqlx::query(
        "INSERT INTO food_aliases (alias, normalized_alias, food_id, source) \
                  VALUES ('coconut milk', 'coconut milk', ?, 'automatic')",
    )
    .bind(milk.id)
    .execute(&pool)
    .await
    .expect("seed corrupt alias");

    let llm = MockFoodLlm::new(MockPlan::Fail);
    let out = resolve_batch(&pool, &llm, &phrases(&["coconut milk"]))
        .await
        .expect("resolve");
    assert_eq!(
        out[0].food_id,
        Some(coconut_milk.id),
        "coconut milk != milk"
    );
    assert_eq!(out[0].canonical_name.as_deref(), Some("coconut milk"));
    assert_eq!(out[0].resolution_source, Some("food"));
    assert!(!out[0].needs_review);
    assert_eq!(llm.calls(), 0);
}

/// Automatic alias creation must not shadow another Food's canonical name;
/// a deliberate user confirmation is still allowed (teaching path).
#[tokio::test]
async fn automatic_alias_may_not_shadow_canonical_name() {
    let pool = test_pool().await;
    let milk = catalog::create_food(&pool, "milk", None, "unknown", None)
        .await
        .expect("milk");
    catalog::create_food(&pool, "coconut milk", None, "unknown", None)
        .await
        .expect("coconut milk");

    let err = catalog::create_alias(&pool, "coconut milk", milk.id, "automatic", false, None)
        .await
        .expect_err("shadowing alias must be rejected");
    assert!(err.to_string().contains("ShadowsCanonical"), "{err}");

    // Deliberate user confirmation overrides (explicit operation).
    let user = catalog::confirm_alias(&pool, "coconut milk", milk.id)
        .await
        .expect("user teaching path");
    assert_eq!(user.food_id, milk.id);
    assert_eq!(user.source, "user");
    assert!(user.confirmed);
}

/* =========================
 * Compound identity guard (audit aliases)
 * ========================= */

/// Every corrupt mapping observed in the audit must now be rejected at
/// creation time (where the corruption is deterministic). Both sides of
/// each pair are seeded: the guard applies once both Foods exist, and a
/// canonical name always outranks any alias afterwards.
#[tokio::test]
async fn audit_bad_aliases_are_rejected() {
    let cases = [
        ("coconut milk", "milk"),
        ("onion powder", "onion"),
        ("almond flour", "flour"),
        ("coconut flour", "flour"),
        ("corn flour", "flour"),
        ("brown sugar", "sugar"),
        ("peanut butter", "butter"),
        ("sweet potato", "potato"),
        ("potato starch", "potato"),
        ("spring onion", "onion"),
        ("apple cider vinegar", "apple"),
        ("sun-dried tomato", "tomato"),
        ("milk", "coconut milk"),
    ];
    for (alias, food_name) in cases {
        let pool = test_pool().await;
        for name in [food_name, alias] {
            catalog::create_food(&pool, name, None, "unknown", None)
                .await
                .unwrap_or_else(|e| panic!("seed '{name}': {e}"));
        }
        let target = food_id(&pool, food_name).await;
        let err = catalog::create_alias(&pool, alias, target, "automatic", false, None)
            .await
            .expect_err("compound/shadow alias must be rejected");
        assert!(
            err.to_string().contains("ShadowsCanonical")
                || err.to_string().contains("CompoundOfOtherFood"),
            "'{alias}' -> '{food_name}': {err}"
        );
    }
}

/// Desired convergence must keep working: pure preparation/size qualifiers
/// may fold onto the base Food.
#[tokio::test]
async fn qualifier_variants_still_converge() {
    let pool = test_pool().await;
    let cumin = catalog::create_food(&pool, "cumin", None, "unknown", None)
        .await
        .expect("cumin");
    let potato = catalog::create_food(&pool, "potato", None, "unknown", None)
        .await
        .expect("potato");
    let shiitake = catalog::create_food(&pool, "shiitake mushrooms", None, "unknown", None)
        .await
        .expect("shiitake");
    let coconut_milk = catalog::create_food(&pool, "coconut milk", None, "unknown", None)
        .await
        .expect("coconut milk");

    catalog::create_alias(&pool, "ground cumin", cumin.id, "automatic", false, None)
        .await
        .expect("ground cumin -> cumin is the desired convergence");
    catalog::create_alias(&pool, "large potatoes", potato.id, "automatic", false, None)
        .await
        .expect("large potatoes -> potato converges");
    catalog::create_alias(
        &pool,
        "dried shiitake mushrooms",
        shiitake.id,
        "automatic",
        false,
        None,
    )
    .await
    .expect("dried shiitake mushrooms -> shiitake mushrooms converges");
    catalog::create_alias(
        &pool,
        "full-fat coconut milk",
        coconut_milk.id,
        "automatic",
        false,
        None,
    )
    .await
    .expect("full-fat coconut milk -> coconut milk converges");
}

/// Pure helper-level checks of the identity guard.
#[test]
fn identity_guard_token_logic() {
    // Mapping a phrase onto the compound Food it names is fine even though
    // the base Food is contained in it.
    let foods = vec![
        (1, "milk".to_string()),
        (2, "coconut milk".to_string()),
        (3, "potato".to_string()),
    ];
    assert!(check_alias_identity("coconut milk", 2, "coconut milk", &foods).is_ok());
    // ...but mapping it onto the base Food is compound corruption.
    assert!(matches!(
        check_alias_identity("coconut milk", 1, "milk", &foods),
        Err(AliasConflict::ShadowsCanonical { .. })
    ));
    // Mapping a compound phrase onto an unrelated compound Food is also
    // rejected (coconut milk -> sweet potato).
    assert!(matches!(
        check_alias_identity("coconut milk", 2, "sweet potato", &foods),
        Err(AliasConflict::CompoundOfOtherFood { .. })
    ));
    // A compound phrase must not alias onto a modifier-only Food name.
    let sweet = vec![(1, "sweet".to_string()), (2, "potato".to_string())];
    assert!(matches!(
        check_alias_identity("sweet potato", 2, "potato", &sweet),
        Err(AliasConflict::CompoundOfOtherFood { .. })
    ));
}

/* =========================
 * Distinctness of audited pairs (resolution level)
 * ========================= */

/// Seed both sides of each audited pair and assert the compound phrase
/// never resolves to the base Food without an LLM.
#[tokio::test]
async fn audited_distinct_pairs_never_auto_merge() {
    let pool = test_pool().await;
    for base in [
        "potato",
        "milk",
        "sugar",
        "butter",
        "onion",
        "flour",
        "rice",
        "tea",
        "spinach",
        "white wine",
        "red bell pepper",
        "sun-dried tomato",
        "tomato",
    ] {
        catalog::create_food(&pool, base, None, "unknown", None)
            .await
            .expect("base food");
    }
    // A few distinct compound Foods that exist in the real catalog.
    for compound in [
        "coconut milk",
        "sweet potato",
        "peanut butter",
        "potato starch",
    ] {
        catalog::create_food(&pool, compound, None, "unknown", None)
            .await
            .expect("compound food");
    }

    let llm = MockFoodLlm::new(MockPlan::Fail);
    let inputs = [
        "coconut milk",
        "onion powder",
        "almond flour",
        "coconut flour",
        "corn flour",
        "quinoa",
        "parmesan",
        "white vinegar",
        "red chili powder",
        "dried thyme",
        "new potatoes",
        "active yeast",
        "sesame seeds",
        "sweet potato",
        "potato starch",
        "peanut butter",
        "brown sugar",
    ];
    let out = resolve_batch(&pool, &llm, &phrases(&inputs))
        .await
        .expect("resolve");

    let expect_exact: &[(&str, &str)] = &[
        ("coconut milk", "coconut milk"),
        ("sweet potato", "sweet potato"),
        ("peanut butter", "peanut butter"),
        ("potato starch", "potato starch"),
    ];
    for (i, phrase) in inputs.iter().enumerate() {
        if let Some((_, canonical)) = expect_exact.iter().find(|(p, _)| p == phrase) {
            assert_eq!(
                out[i].canonical_name.as_deref(),
                Some(*canonical),
                "'{phrase}' must resolve to its own Food"
            );
        } else {
            assert_ne!(
                out[i].canonical_name.as_deref(),
                Some(*phrase),
                "'{phrase}' must not silently merge into a wrong Food"
            );
            assert!(
                out[i].food_id.is_none() || out[i].needs_review,
                "'{phrase}' resolved without review"
            );
        }
    }

    // Explicit cross-checks for the headline cases.
    let milk = food_id(&pool, "milk").await;
    let coconut = food_id(&pool, "coconut milk").await;
    assert_ne!(milk, coconut);
    assert_eq!(out[0].food_id, Some(coconut));

    let potato = food_id(&pool, "potato").await;
    let sweet = food_id(&pool, "sweet potato").await;
    assert_eq!(out[13].food_id, Some(sweet));
    assert_ne!(out[13].food_id, Some(potato));
}

/* =========================
 * Batch response association (work-item keys)
 * ========================= */

fn two_input_request() -> LlmResolveRequest {
    LlmResolveRequest {
        inputs: vec![
            LlmInput {
                key: "wi0000-aaaa".to_string(),
                phrase: "milk".to_string(),
                candidates: vec![LlmCandidate {
                    food_id: 11,
                    name: "milk".to_string(),
                    matched_via: None,
                }],
            },
            LlmInput {
                key: "wi0001-bbbb".to_string(),
                phrase: "coconut milk".to_string(),
                candidates: vec![
                    LlmCandidate {
                        food_id: 11,
                        name: "milk".to_string(),
                        matched_via: None,
                    },
                    LlmCandidate {
                        food_id: 74,
                        name: "coconut milk".to_string(),
                        matched_via: None,
                    },
                ],
            },
        ],
        categories: vec![(1, "Other".to_string())],
    }
}

#[test]
fn response_entries_validate_against_their_own_candidates() {
    let req = two_input_request();

    // Input 1 returns input 0's only candidate (11) — for input 0 that is
    // valid; for input 1 it is also a candidate, so this is admissible.
    // Input 0 returns 74, which is NOT among input 0's candidates.
    let value = serde_json::json!({
        "results": [
            {"key": "wi0000-aaaa", "input_index": 0, "food_id": 74,
             "new_food": null, "qualifiers": [], "needs_review": false},
            {"key": "wi0001-bbbb", "input_index": 1, "food_id": 74,
             "new_food": null, "qualifiers": [], "needs_review": false}
        ]
    });
    let out = resolver::parse_llm_response(&value, &req);
    assert_eq!(out.len(), 2);
    let for_first = out.iter().find(|r| r.input_index == 0).expect("entry 0");
    assert_eq!(
        for_first.food_id, None,
        "candidate of another input is rejected"
    );
    let for_second = out.iter().find(|r| r.input_index == 1).expect("entry 1");
    assert_eq!(for_second.food_id, Some(74));
}

#[test]
fn unknown_or_contradictory_keys_are_ignored() {
    let req = two_input_request();

    // Unknown key → dropped even though the index would be in range.
    let value = serde_json::json!({
        "results": [
            {"key": "wi9999-zzzz", "input_index": 0, "food_id": 11,
             "new_food": null, "qualifiers": [], "needs_review": false}
        ]
    });
    assert!(resolver::parse_llm_response(&value, &req).is_empty());

    // Key and index disagree → dropped.
    let value = serde_json::json!({
        "results": [
            {"key": "wi0000-aaaa", "input_index": 1, "food_id": 11,
             "new_food": null, "qualifiers": [], "needs_review": false}
        ]
    });
    assert!(resolver::parse_llm_response(&value, &req).is_empty());

    // Index-only fallback still works for models that drop the key.
    let value = serde_json::json!({
        "results": [
            {"input_index": 1, "food_id": 74, "new_food": null,
             "qualifiers": [], "needs_review": false}
        ]
    });
    let out = resolver::parse_llm_response(&value, &req);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].input_index, 1);
    assert_eq!(out[0].food_id, Some(74));
}

#[test]
fn duplicate_entries_for_one_work_item_are_refused() {
    let req = two_input_request();
    let value = serde_json::json!({
        "results": [
            {"key": "wi0001-bbbb", "input_index": 1, "food_id": 11,
             "new_food": null, "qualifiers": [], "needs_review": false},
            {"key": "wi0001-bbbb", "input_index": 1, "food_id": 74,
             "new_food": null, "qualifiers": [], "needs_review": false}
        ]
    });
    let out = resolver::parse_llm_response(&value, &req);
    let entry = out.iter().find(|r| r.input_index == 1).expect("entry kept");
    assert_eq!(
        entry.food_id, None,
        "ambiguous duplicate must not pick a food"
    );
    assert!(entry.needs_review);
}

#[test]
fn work_item_keys_are_stable_and_distinct() {
    let a = resolver::work_item_key(0);
    let b = resolver::work_item_key(1);
    let c = resolver::work_item_key(0);
    assert_eq!(a, c, "same index + phrase → same key");
    assert_ne!(a, b, "different work items → different keys");
}

/* =========================
 * Same-run reconciliation (zero extra LLM calls)
 * ========================= */

/// Input 0 ("salt") is unresolved by the LLM; input 1 creates the canonical
/// Food `salt`. The reconciliation pass must then resolve input 0 locally
/// with no additional LLM call.
#[tokio::test]
async fn reconciliation_resolves_earlier_phrases_after_later_batches() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CreateLaterMock {
        calls: AtomicUsize,
    }
    impl FoodLlm for CreateLaterMock {
        async fn resolve(&self, req: &LlmResolveRequest) -> anyhow::Result<Vec<LlmResultItem>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(req
                .inputs
                .iter()
                .enumerate()
                .map(|(i, input)| LlmResultItem {
                    input_index: i,
                    food_id: None,
                    new_food: (input.phrase == "kosher salt").then(|| LlmNewFood {
                        canonical_name: "salt".to_string(),
                        category_id: None,
                    }),
                    qualifiers: Vec::new(),
                    needs_review: input.phrase == "salt",
                })
                .collect())
        }
    }

    let pool = test_pool().await;
    let mock = CreateLaterMock {
        calls: AtomicUsize::new(0),
    };

    let out = resolve_batch(&pool, &mock, &phrases(&["salt", "kosher salt"]))
        .await
        .expect("resolve");
    assert_eq!(
        mock.calls.load(Ordering::SeqCst),
        1,
        "exactly one LLM batch"
    );

    let salt = food_id(&pool, "salt").await;
    assert_eq!(out[1].food_id, Some(salt), "later batch created the Food");
    assert_eq!(
        out[0].food_id,
        Some(salt),
        "earlier occurrence reconciled locally in the same run"
    );
    assert_eq!(out[0].resolution_source, Some("food"));
    assert!(
        !out[0].needs_review,
        "reconciled phrase is not needs_review"
    );
}

/* =========================
 * Category creation at source
 * ========================= */

/// A valid new-food result persists the category with `category_source =
/// llm`; an invalid category is dropped (never silently `Other`).
#[tokio::test]
async fn new_food_category_provenance() {
    let pool = test_pool().await;
    let pantry = catalog::list_categories(&pool)
        .await
        .expect("categories")
        .iter()
        .find(|(_, n)| n == "Pantry")
        .map(|(id, _)| *id)
        .expect("Pantry exists");

    let llm = MockFoodLlm::new(MockPlan::NewFood {
        name: "gochujang",
        category: Some(pantry),
    });
    let out = resolve_batch(&pool, &llm, &phrases(&["gochujang"]))
        .await
        .expect("resolve");
    let food = catalog::get_food_by_id(&pool, out[0].food_id.expect("food"))
        .await
        .expect("reload")
        .expect("exists");
    assert_eq!(food.category_id, Some(pantry));
    assert_eq!(food.category_source, "llm");

    // Invalid category id: food stays uncategorized, never coerced to Other.
    let llm = MockFoodLlm::new(MockPlan::NewFood {
        name: "ajvar",
        category: Some(9999),
    });
    let out = resolve_batch(&pool, &llm, &phrases(&["ajvar"]))
        .await
        .expect("resolve");
    let food = catalog::get_food_by_id(&pool, out[0].food_id.expect("food"))
        .await
        .expect("reload")
        .expect("exists");
    assert_eq!(food.category_id, None, "invalid category dropped");
    assert_eq!(food.category_source, "llm");

    let (other,): (i64,) =
        sqlx::query_as("SELECT id FROM shopping_categories WHERE name = 'Other'")
            .fetch_one(&pool)
            .await
            .expect("Other exists");
    assert_ne!(food.category_id, Some(other));
}

/// The shipped resolver prompt must ask for a category on every new food.
#[test]
fn resolver_prompt_requires_category() {
    let prompt = crate::config::DEFAULT_SYSTEM_PROMPT_FOOD_RESOLVER;
    assert!(
        prompt.contains("\"category_id\""),
        "example must show category_id"
    );
    assert!(prompt.contains("ALWAYS set new_food.category_id"));
}

/* =========================
 * Garbage canonical names (audit regressions)
 * ========================= */

#[tokio::test]
async fn pathological_food_names_are_rejected() {
    let pool = test_pool().await;
    let garbage = [
        "1. remove quantities: \"3\" → \"\"\n2. singular form: \"buns\" → \"bun\"",
        "here's the normalized output for \"kale\":\n\n```json\n\"kale\"\n```",
        "to normalize the ingredient name \"black energy drink\", we follow these steps:\n\n1. remove quantities - none present",
        "sweet",
        "fresh",
        "serve with",
    ];
    for name in garbage {
        assert!(
            catalog::create_food(&pool, name, None, "llm", None)
                .await
                .is_err(),
            "'{name}' must be rejected"
        );
    }

    // Identity-bearing compounds must remain creatable.
    for name in [
        "sweet potato",
        "brown sugar",
        "red wine",
        "coconut milk",
        "sweet chili sauce",
    ] {
        catalog::create_food(&pool, name, None, "llm", None)
            .await
            .unwrap_or_else(|e| panic!("'{name}' must be accepted: {e}"));
    }
}

/* =========================
 * Candidate retrieval stays candidate-only
 * ========================= */

#[test]
fn candidate_retrieval_never_creates_identity() {
    let snapshot = CatalogSnapshot {
        foods: vec![CatalogFoodRef {
            id: 1,
            canonical_name: "potato".to_string(),
            normalized_name: "potato".to_string(),
        }],
        aliases: vec![],
    };
    let candidates = resolver::candidate_matches(&snapshot, "large potatoes", CANDIDATE_LIMIT);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].food_id, 1);
}
