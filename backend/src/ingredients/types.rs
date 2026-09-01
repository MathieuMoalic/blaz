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
