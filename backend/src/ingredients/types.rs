//! Shared row and DTO types for the ingredient pipeline.

/// A canonical food row.
#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)] // consumed from commit 4 (resolver) onwards
#[allow(clippy::struct_field_names)]
pub struct Food {
    pub id: i64,
    pub canonical_name: String,
    pub normalized_name: String,
    pub category_id: Option<i64>,
    pub category_source: String,
    pub category_confidence: Option<f64>,
}

/// A food alias row.
#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)] // consumed from commit 4 (resolver) onwards
#[allow(clippy::struct_field_names)]
pub struct FoodAlias {
    pub id: i64,
    pub alias: String,
    pub normalized_alias: String,
    pub food_id: i64,
    pub source: String,
    pub confidence: Option<f64>,
    pub confirmed: bool,
}

/// One food search / autocomplete result.
#[derive(Debug, Clone)]
#[allow(dead_code)] // consumed from commit 9 (foods endpoint) onwards
#[allow(clippy::struct_field_names)]
pub struct FoodSearchRow {
    pub id: i64,
    pub canonical_name: String,
    pub category_id: Option<i64>,
    pub category: Option<String>,
    pub matched_alias: Option<String>,
}

/* ---------- resolver DTOs ---------- */

/// Result of resolving one ingredient phrase to a canonical Food.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // consumed from commit 6 (imports) onwards
#[allow(clippy::struct_field_names)]
pub struct ResolutionOutcome {
    pub food_id: Option<i64>,
    pub canonical_name: Option<String>,
    pub qualifiers: Vec<String>,
    /// `confirmed_alias` | `alias` | `food` | `deterministic` | `llm` |
    /// `new_food` | `unresolved`
    pub resolution_source: Option<&'static str>,
    pub resolution_confidence: Option<f64>,
    pub needs_review: bool,
}

impl ResolutionOutcome {
    /// No food identity; flagged for user review.
    #[must_use]
    pub const fn unresolved() -> Self {
        Self {
            food_id: None,
            canonical_name: None,
            qualifiers: Vec::new(),
            resolution_source: Some("unresolved"),
            resolution_confidence: None,
            needs_review: true,
        }
    }
}

/// Candidate Food offered to the LLM for one input phrase.
#[derive(Debug, Clone)]
#[allow(dead_code)] // consumed from commit 6 (imports) onwards
pub struct LlmCandidate {
    pub food_id: i64,
    pub name: String,
    pub matched_via: Option<String>,
}

/// One unresolved input phrase plus its candidate Foods.
#[derive(Debug, Clone)]
#[allow(dead_code)] // consumed from commit 6 (imports) onwards
pub struct LlmInput {
    pub phrase: String,
    pub candidates: Vec<LlmCandidate>,
}

/// One batched resolution request for the LLM.
#[derive(Debug, Clone)]
#[allow(dead_code)] // consumed from commit 6 (imports) onwards
pub struct LlmResolveRequest {
    pub inputs: Vec<LlmInput>,
    pub categories: Vec<(i64, String)>,
}

/// A new Food proposed by the LLM.
#[derive(Debug, Clone)]
#[allow(dead_code)] // consumed from commit 6 (imports) onwards
pub struct LlmNewFood {
    pub canonical_name: String,
    pub category_id: Option<i64>,
}

/// One validated LLM decision for a single input.
#[derive(Debug, Clone)]
#[allow(dead_code)] // consumed from commit 6 (imports) onwards
pub struct LlmResultItem {
    pub input_index: usize,
    pub food_id: Option<i64>,
    pub new_food: Option<LlmNewFood>,
    pub qualifiers: Vec<String>,
    pub needs_review: bool,
}

/* ---------- catalog snapshot (candidate retrieval) ---------- */

/// Compact food listing for candidate retrieval.
#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)] // consumed from commit 6 (imports) onwards
pub struct CatalogFoodRef {
    pub id: i64,
    pub canonical_name: String,
    pub normalized_name: String,
}

/// Compact alias listing for candidate retrieval.
#[derive(Debug, Clone, sqlx::FromRow)]
#[allow(dead_code)] // consumed from commit 6 (imports) onwards
pub struct CatalogAliasRef {
    pub alias: String,
    pub normalized_alias: String,
    pub food_id: i64,
}

/// A point-in-time view of foods + aliases for candidate matching.
#[derive(Debug, Clone)]
#[allow(dead_code)] // consumed from commit 6 (imports) onwards
pub struct CatalogSnapshot {
    pub foods: Vec<CatalogFoodRef>,
    pub aliases: Vec<CatalogAliasRef>,
}
