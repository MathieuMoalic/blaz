//! Food catalog persistence: canonical foods, aliases, and categories.

use std::collections::HashSet;

use sqlx::SqlitePool;

use crate::ingredients::types::{
    CatalogAliasRef, CatalogFoodRef, CatalogSnapshot, Food, FoodAlias, FoodSearchRow,
};
use crate::units::normalize_name;

const FOOD_COLS: &str =
    "id, canonical_name, normalized_name, category_id, category_source, category_confidence";
const ALIAS_COLS: &str = "id, alias, normalized_alias, food_id, source, confidence, confirmed";

/// A confirmed incoming alias mapping replaces an existing unconfirmed
/// non-user mapping; every other conflict keeps the existing row.
const ALIAS_WINNER: &str =
    "excluded.confirmed = 1 AND food_aliases.confirmed = 0 AND food_aliases.source <> 'user'";

/* =========================
 * Catalog integrity guards
 * ========================= */

/// Quantity/unit noise tokens that never carry identity.
const NOISE_TOKENS: &[&str] = &[
    "g",
    "kg",
    "mg",
    "ml",
    "l",
    "cl",
    "dl",
    "oz",
    "ounce",
    "ounces",
    "lb",
    "lbs",
    "pound",
    "pounds",
    "tsp",
    "tbsp",
    "tbs",
    "teaspoon",
    "teaspoons",
    "tablespoon",
    "tablespoons",
    "cup",
    "cups",
    "can",
    "cans",
    "clove",
    "cloves",
    "pinch",
    "handful",
    "handfuls",
    "pack",
    "packet",
    "packets",
    "piece",
    "pieces",
    "slice",
    "slices",
    "sheet",
    "sheets",
    "skewer",
    "skewers",
    "stalk",
    "stalks",
    "stick",
    "sticks",
    "sprig",
    "sprigs",
    "dash",
    "splash",
    "bunch",
    "bunches",
    "leaf",
    "leaves",
    "grain",
    "grains",
    "pod",
    "pods",
    "head",
    "heads",
    "fillet",
    "fillets",
];

/// Preparation / size / ripeness qualifiers that may be stripped when an
/// alias converges onto a base Food (`ground cumin` → `cumin`).
///
/// Deliberately *not* here: identity-bearing words such as `sweet`,
/// `brown`, `red`, `coconut`, `almond`, `spring` — compounds using those
/// must never alias onto the base food automatically.
const QUALIFIER_TOKENS: &[&str] = &[
    "fresh", "dried", "dry", "frozen", "canned", "ground", "chopped", "diced", "sliced", "minced",
    "peeled", "grated", "shredded", "crushed", "cubed", "cooked", "raw", "organic", "ripe",
    "whole", "large", "small", "medium", "big", "little", "extra", "virgin", "baby", "rolled",
    "instant", "active", "finely", "roughly", "thinly", "rinsed", "drained", "softened", "heaping",
    "level", "rounded", "packed", "natural", "toasted", "roasted", "uncooked", "full-fat",
    "low-fat", "lite", "unsalted", "salted", "skinless", "boneless",
];

/// Generic modifier words that must never form a canonical Food on their
/// own (audit regression: LLM created Food "sweet" for sweet-potato
/// phrases). Includes bare function/instruction words seen in the audit.
const MODIFIER_ONLY_TOKENS: &[&str] = &[
    "sweet",
    "sour",
    "spicy",
    "hot",
    "cold",
    "warm",
    "cool",
    "fresh",
    "dried",
    "frozen",
    "canned",
    "large",
    "small",
    "medium",
    "big",
    "little",
    "mixed",
    "raw",
    "ripe",
    "organic",
    "whole",
    "ground",
    "cooked",
    "chopped",
    "extra",
    "optional",
    "garnish",
    "topping",
    "toppings",
    "filling",
    "fillings",
    "sauce",
    "seasoning",
    "mixture",
    "batter",
    "wash",
    "liquid",
    "water",
    "serve",
    "serving",
    "taste",
    "for",
    "and",
    "or",
    "with",
    "the",
    "of",
    "a",
    "to",
    "in",
    "into",
    "plus",
    "about",
];

fn singular_token(word: &str) -> &str {
    if word.len() <= 3 {
        return word;
    }
    if let Some(stem) = word.strip_suffix("ies") {
        return stem; // close enough for token comparison
    }
    if let Some(stem) = word.strip_suffix("es") {
        return stem;
    }
    if let Some(stem) = word.strip_suffix('s')
        && !word.ends_with("ss")
    {
        return stem;
    }
    word
}

/// Content tokens of a normalized phrase, lowercased, with plural noise
/// singularized, quantity/unit noise and qualifiers removed. Returns
/// `None` for tokens with digits or single characters (noise).
fn content_tokens(normalized: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for word in normalized.split_whitespace() {
        if word.chars().any(|c| c.is_ascii_digit()) || word.len() <= 1 {
            continue;
        }
        let sing = singular_token(word);
        if NOISE_TOKENS.contains(&sing) || NOISE_TOKENS.contains(&word) {
            continue;
        }
        if QUALIFIER_TOKENS.contains(&sing) || QUALIFIER_TOKENS.contains(&word) {
            continue;
        }
        out.push(sing.to_string());
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Why an automatic alias mapping was rejected.
#[derive(Debug, PartialEq, Eq)]
pub enum AliasConflict {
    /// The alias text is the canonical name of a different Food.
    ShadowsCanonical { other_food_id: i64 },
    /// The alias is a qualified/compound variant of a different Food
    /// (e.g. `coconut milk` for Food `milk`): automatic creation would
    /// corrupt that Food's identity.
    CompoundOfOtherFood { other_food_id: i64 },
}

/// Check that an automatic alias mapping preserves catalog identity.
///
/// 1. the alias text must not be another Food's canonical name;
/// 2. the alias must not be a compound variant of a *different* Food
///    (`coconut milk` → Food `milk`, `onion powder` → Food `onion`,
///    `almond flour` → Food `flour`). Pure qualifier variants converge
///    (`ground cumin` → `cumin`), and a mapping onto the compound Food
///    itself is fine (`coconut milk` → Food `coconut milk`,
///    `full-fat coconut milk` → Food `coconut milk`).
///
/// User-confirmed mappings bypass this check: they are deliberate
/// teaching decisions.
pub fn check_alias_identity(
    alias_normalized: &str,
    food_id: i64,
    target_normalized: &str,
    foods: &[(i64, String)], // (id, normalized_name)
) -> Result<(), AliasConflict> {
    let alias_tokens = content_tokens(alias_normalized);
    let target_tokens = content_tokens(target_normalized);
    // A mapping whose content tokens equal the target's (it differs only
    // by qualifiers/noise) always converges onto the target legitimately.
    let same_as_target = target_tokens.is_some() && alias_tokens == target_tokens;
    for (other_id, other_name) in foods {
        if *other_id == food_id {
            continue;
        }
        if *other_name == alias_normalized {
            return Err(AliasConflict::ShadowsCanonical {
                other_food_id: *other_id,
            });
        }
        let (Some(alias), Some(other_tokens)) = (alias_tokens.as_ref(), content_tokens(other_name))
        else {
            continue;
        };
        // The alias is a compound of the other Food if that Food's whole
        // content name appears inside it, and the mapping does not simply
        // converge onto the (more specific) target Food.
        if !same_as_target && other_tokens.iter().all(|t| alias.contains(t)) {
            return Err(AliasConflict::CompoundOfOtherFood {
                other_food_id: *other_id,
            });
        }
    }
    Ok(())
}

/// Detect instruction-like / pathological canonical names observed in the
/// migration audit (LLM reasoning text stored as a Food name, modifier-only
/// names).
pub fn is_pathological_food_name(canonical_name: &str, normalized: &str) -> Option<&'static str> {
    let lower = canonical_name.to_lowercase();
    if lower.contains("```")
        || lower.contains("remove quantities")
        || lower.contains("normalized output")
        || lower.contains("normalize the ingredient")
        || lower.starts_with("here's")
        || lower.starts_with("step ")
        || canonical_name.matches('\n').count() >= 2
    {
        return Some("instruction-like text");
    }
    if normalized.chars().count() > 80 {
        return Some("unreasonably long");
    }
    // Modifier-only names: every content token is a generic modifier.
    let words: Vec<&str> = normalized.split_whitespace().collect();
    if !words.is_empty()
        && words.iter().all(|w| {
            MODIFIER_ONLY_TOKENS.contains(w) || MODIFIER_ONLY_TOKENS.contains(&singular_token(w))
        })
    {
        return Some("generic modifier without an ingredient");
    }
    None
}

/* =========================
 * Lookups
 * ========================= */

/// Fetch a food by ID.
///
/// # Errors
///
/// Returns an error if the database lookup fails.
pub async fn get_food_by_id(pool: &SqlitePool, id: i64) -> anyhow::Result<Option<Food>> {
    let sql = format!("SELECT {FOOD_COLS} FROM foods WHERE id = ?");
    Ok(sqlx::query_as::<_, Food>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

/// Fetch a food by name (any casing/whitespace), matching
/// `foods.normalized_name` after normalization.
///
/// # Errors
///
/// Returns an error if the database lookup fails.
pub async fn get_food_by_name(pool: &SqlitePool, name: &str) -> anyhow::Result<Option<Food>> {
    let normalized = normalize_name(name);
    if normalized.is_empty() {
        return Ok(None);
    }
    let sql = format!("SELECT {FOOD_COLS} FROM foods WHERE normalized_name = ?");
    Ok(sqlx::query_as::<_, Food>(&sql)
        .bind(normalized)
        .fetch_optional(pool)
        .await?)
}

/// Fetch an alias by name (any casing/whitespace), matching
/// `food_aliases.normalized_alias` after normalization.
///
/// # Errors
///
/// Returns an error if the database lookup fails.
pub async fn find_alias(pool: &SqlitePool, name: &str) -> anyhow::Result<Option<FoodAlias>> {
    let normalized = normalize_name(name);
    if normalized.is_empty() {
        return Ok(None);
    }
    let sql = format!("SELECT {ALIAS_COLS} FROM food_aliases WHERE normalized_alias = ?");
    Ok(sqlx::query_as::<_, FoodAlias>(&sql)
        .bind(normalized)
        .fetch_optional(pool)
        .await?)
}

/* =========================
 * Mutations
 * ========================= */

/// Create a food, race-safe: concurrent creations of the same normalized
/// name resolve to the same row (the `UNIQUE` constraint on
/// `normalized_name` picks the winner). Returns the resulting food — newly
/// created or already existing.
///
/// # Errors
///
/// Returns an error when the name normalizes to an empty string or the
/// database write fails.
pub async fn create_food(
    pool: &SqlitePool,
    canonical_name: &str,
    category_id: Option<i64>,
    category_source: &str,
    category_confidence: Option<f64>,
) -> anyhow::Result<Food> {
    let canonical_name = canonical_name.trim();
    let normalized = normalize_name(canonical_name);
    if normalized.is_empty() {
        anyhow::bail!("food name '{canonical_name}' normalizes to an empty string");
    }
    if let Some(reason) = is_pathological_food_name(canonical_name, &normalized) {
        anyhow::bail!("rejecting pathological food name {canonical_name:?}: {reason}");
    }

    sqlx::query(
        r"
        INSERT INTO foods (canonical_name, normalized_name, category_id, category_source, category_confidence)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(normalized_name) DO NOTHING
        ",
    )
    .bind(canonical_name)
    .bind(&normalized)
    .bind(category_id)
    .bind(category_source)
    .bind(category_confidence)
    .execute(pool)
    .await?;

    let sql = format!("SELECT {FOOD_COLS} FROM foods WHERE normalized_name = ?");
    Ok(sqlx::query_as::<_, Food>(&sql)
        .bind(&normalized)
        .fetch_one(pool)
        .await?)
}

/// Upsert one alias mapping.
///
/// Fresh aliases are inserted. On conflict, a *confirmed* incoming mapping
/// replaces an existing unconfirmed non-user mapping; anything else leaves
/// the existing row untouched (user and confirmed mappings are never
/// silently rewritten — a kept conflicting mapping is logged, never
/// corrupted). Automatic mappings must additionally pass the identity
/// guard: they may not shadow another Food's canonical name and may not be
/// a compound variant of a different Food (`coconut milk` → Food `milk`).
/// User-confirmed mappings bypass the identity guard.
///
/// # Errors
///
/// Returns an error when the alias normalizes to an empty string, the food
/// does not exist, an automatic mapping would corrupt Food identity, or the
/// database write fails.
pub async fn create_alias(
    pool: &SqlitePool,
    alias: &str,
    food_id: i64,
    source: &str,
    confirmed: bool,
    confidence: Option<f64>,
) -> anyhow::Result<FoodAlias> {
    let alias = alias.trim();
    let normalized = normalize_name(alias);
    if normalized.is_empty() {
        anyhow::bail!("alias '{alias}' normalizes to an empty string");
    }
    let food = get_food_by_id(pool, food_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("food {food_id} does not exist"))?;

    let user_confirmed = confirmed && source == "user";
    if !user_confirmed {
        let foods: Vec<(i64, String)> = sqlx::query_as("SELECT id, normalized_name FROM foods")
            .fetch_all(pool)
            .await?;
        if let Err(conflict) =
            check_alias_identity(&normalized, food_id, &food.normalized_name, &foods)
        {
            return Err(anyhow::anyhow!(
                "alias '{alias}' for food '{}' (#{}): {conflict:?}",
                food.canonical_name,
                food.id
            ));
        }
    }

    let sql = format!(
        r"
        INSERT INTO food_aliases (alias, normalized_alias, food_id, source, confirmed, confidence)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(normalized_alias) DO UPDATE SET
            alias = CASE WHEN {ALIAS_WINNER} THEN excluded.alias ELSE food_aliases.alias END,
            food_id = CASE WHEN {ALIAS_WINNER} THEN excluded.food_id ELSE food_aliases.food_id END,
            source = CASE WHEN {ALIAS_WINNER} THEN excluded.source ELSE food_aliases.source END,
            confirmed = CASE WHEN {ALIAS_WINNER} THEN excluded.confirmed ELSE food_aliases.confirmed END,
            confidence = CASE WHEN {ALIAS_WINNER} THEN excluded.confidence ELSE food_aliases.confidence END
        "
    );

    sqlx::query(&sql)
        .bind(alias)
        .bind(&normalized)
        .bind(food_id)
        .bind(source)
        .bind(confirmed)
        .bind(confidence)
        .execute(pool)
        .await?;

    let select = format!("SELECT {ALIAS_COLS} FROM food_aliases WHERE normalized_alias = ?");
    let row = sqlx::query_as::<_, FoodAlias>(&select)
        .bind(&normalized)
        .fetch_one(pool)
        .await?;
    if row.food_id != food_id {
        // Never silently overwrite: the existing mapping stays authoritative.
        tracing::warn!(
            alias = %row.alias,
            existing_food_id = row.food_id,
            requested_food_id = food_id,
            requested_source = source,
            confirmed,
            "alias mapping conflict; keeping the existing mapping"
        );
    }
    Ok(row)
}

/// User-confirm that `alias` means `food_id`.
///
/// This is the teaching path: the mapping is stored with `source = 'user'`
/// and `confirmed = 1` and is never overwritten by automatic resolution. A
/// later confirmation for the same alias replaces the previous user mapping
/// (the latest user choice wins).
///
/// # Errors
///
/// Returns an error when the alias normalizes to an empty string, the food
/// does not exist, or the database write fails.
pub async fn confirm_alias(
    pool: &SqlitePool,
    alias: &str,
    food_id: i64,
) -> anyhow::Result<FoodAlias> {
    let alias = alias.trim();
    let normalized = normalize_name(alias);
    if normalized.is_empty() {
        anyhow::bail!("alias '{alias}' normalizes to an empty string");
    }
    if get_food_by_id(pool, food_id).await?.is_none() {
        anyhow::bail!("food {food_id} does not exist");
    }

    sqlx::query(
        r"
        INSERT INTO food_aliases (alias, normalized_alias, food_id, source, confirmed)
        VALUES (?, ?, ?, 'user', 1)
        ON CONFLICT(normalized_alias) DO UPDATE SET
            alias = excluded.alias,
            food_id = excluded.food_id,
            source = 'user',
            confirmed = 1,
            confidence = NULL
        ",
    )
    .bind(alias)
    .bind(&normalized)
    .bind(food_id)
    .execute(pool)
    .await?;

    let select = format!("SELECT {ALIAS_COLS} FROM food_aliases WHERE normalized_alias = ?");
    Ok(sqlx::query_as::<_, FoodAlias>(&select)
        .bind(&normalized)
        .fetch_one(pool)
        .await?)
}

/// Set a food's default category.
///
/// Automatic callers are refused while `category_source = 'user'` (returns
/// `Ok(false)` without writing). Pass `force = true` for explicit user
/// actions, which always applies.
///
/// # Errors
///
/// Returns an error when the food or the target category does not exist, or
/// the database write fails.
pub async fn set_food_category(
    pool: &SqlitePool,
    food_id: i64,
    category_id: Option<i64>,
    source: &str,
    confidence: Option<f64>,
    force: bool,
) -> anyhow::Result<bool> {
    if get_food_by_id(pool, food_id).await?.is_none() {
        anyhow::bail!("food {food_id} does not exist");
    }

    if let Some(cid) = category_id {
        let known: Option<i64> =
            sqlx::query_scalar("SELECT id FROM shopping_categories WHERE id = ?")
                .bind(cid)
                .fetch_optional(pool)
                .await?;
        if known.is_none() {
            anyhow::bail!("category {cid} does not exist");
        }
    }

    let sql = if force {
        "UPDATE foods \
         SET category_id = ?, category_source = ?, category_confidence = ?, updated_at = unixepoch() \
         WHERE id = ?"
    } else {
        "UPDATE foods \
         SET category_id = ?, category_source = ?, category_confidence = ?, updated_at = unixepoch() \
         WHERE id = ? AND category_source <> 'user'"
    };

    let res = sqlx::query(sql)
        .bind(category_id)
        .bind(source)
        .bind(confidence)
        .bind(food_id)
        .execute(pool)
        .await?;

    let applied = res.rows_affected() == 1;
    if !applied {
        tracing::debug!(
            food_id,
            "food category is user-protected; automatic update skipped"
        );
    }
    Ok(applied)
}

/* =========================
 * Search
 * ========================= */

type NameHitRow = (i64, String, String, Option<i64>, Option<String>);
type AliasHitRow = (i64, String, String, Option<i64>, Option<String>, String);

fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn match_rank(normalized: &str, needle: &str) -> u8 {
    if normalized == needle {
        0
    } else if normalized.starts_with(needle) {
        1
    } else {
        2
    }
}

/// Search canonical food names and aliases for `query` (case-insensitive
/// substring on normalized text).
///
/// Exact matches rank first, then prefixes, then substrings; name hits rank
/// before alias hits; ties break alphabetically. A food matching through
/// both name and alias appears only once.
///
/// # Errors
///
/// Returns an error if a database lookup fails.
#[allow(dead_code)] // consumed from commit 9 (foods endpoint) onwards
pub async fn search_foods(
    pool: &SqlitePool,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<FoodSearchRow>> {
    let needle = normalize_name(query);
    if needle.is_empty() {
        return Ok(Vec::new());
    }
    let pattern = format!("%{}%", escape_like(&needle));

    let name_rows: Vec<NameHitRow> = sqlx::query_as(
        "SELECT f.id, f.canonical_name, f.normalized_name, f.category_id, c.name \
         FROM foods f \
         LEFT JOIN shopping_categories c ON f.category_id = c.id \
         WHERE f.normalized_name LIKE ? ESCAPE '\\' \
         LIMIT 50",
    )
    .bind(&pattern)
    .fetch_all(pool)
    .await?;

    let alias_rows: Vec<AliasHitRow> = sqlx::query_as(
        "SELECT f.id, f.canonical_name, fa.normalized_alias, f.category_id, c.name, fa.alias \
         FROM food_aliases fa \
         JOIN foods f ON fa.food_id = f.id \
         LEFT JOIN shopping_categories c ON f.category_id = c.id \
         WHERE fa.normalized_alias LIKE ? ESCAPE '\\' \
         LIMIT 50",
    )
    .bind(&pattern)
    .fetch_all(pool)
    .await?;

    let mut hits: Vec<(u8, u8, String, FoodSearchRow)> = Vec::new();
    for (id, canonical_name, normalized_name, category_id, category) in name_rows {
        hits.push((
            match_rank(&normalized_name, &needle),
            0,
            canonical_name.clone(),
            FoodSearchRow {
                id,
                canonical_name,
                category_id,
                category,
                matched_alias: None,
            },
        ));
    }
    for (id, canonical_name, normalized_alias, category_id, category, alias) in alias_rows {
        hits.push((
            match_rank(&normalized_alias, &needle),
            1,
            canonical_name.clone(),
            FoodSearchRow {
                id,
                canonical_name,
                category_id,
                category,
                matched_alias: Some(alias),
            },
        ));
    }

    hits.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });

    let mut seen: HashSet<i64> = HashSet::new();
    let mut results: Vec<FoodSearchRow> = Vec::new();
    for (_, _, _, row) in hits {
        if seen.insert(row.id) {
            results.push(row);
            if results.len() >= limit {
                break;
            }
        }
    }
    Ok(results)
}

/* =========================
 * Categories + catalog snapshot
 * ========================= */

/// All shopping categories as `(id, name)`, ordered by `sort_order`.
///
/// # Errors
///
/// Returns an error if the database lookup fails.
pub async fn list_categories(pool: &SqlitePool) -> anyhow::Result<Vec<(i64, String)>> {
    Ok(
        sqlx::query_as("SELECT id, name FROM shopping_categories ORDER BY sort_order")
            .fetch_all(pool)
            .await?,
    )
}

/// Load a snapshot of all foods + aliases for candidate retrieval.
///
/// # Errors
///
/// Returns an error if a database lookup fails.
pub async fn load_catalog_snapshot(pool: &SqlitePool) -> anyhow::Result<CatalogSnapshot> {
    let foods: Vec<CatalogFoodRef> =
        sqlx::query_as("SELECT id, canonical_name, normalized_name FROM foods")
            .fetch_all(pool)
            .await?;
    let aliases: Vec<CatalogAliasRef> =
        sqlx::query_as("SELECT alias, normalized_alias, food_id FROM food_aliases")
            .fetch_all(pool)
            .await?;
    Ok(CatalogSnapshot { foods, aliases })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> SqlitePool {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        crate::db::MIGRATOR.run(&pool).await.expect("migrations");
        pool
    }

    async fn seed_food(pool: &SqlitePool, canonical_name: &str) -> i64 {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO foods (canonical_name, normalized_name) VALUES (?, ?) RETURNING id",
        )
        .bind(canonical_name)
        .bind(normalize_name(canonical_name))
        .fetch_one(pool)
        .await
        .expect("insert food");
        id
    }

    async fn category_id(pool: &SqlitePool, name: &str) -> i64 {
        sqlx::query_as::<_, (i64,)>("SELECT id FROM shopping_categories WHERE name = ?")
            .bind(name)
            .fetch_one(pool)
            .await
            .expect("category exists")
            .0
    }

    /* ---------- catalog lookups & creation ---------- */

    #[tokio::test]
    async fn create_food_roundtrip_and_get() {
        let pool = test_pool().await;
        let vegetables = category_id(&pool, "Vegetables").await;

        let food = create_food(&pool, "  Potato ", Some(vegetables), "llm", Some(0.9))
            .await
            .expect("create food");
        assert_eq!(food.canonical_name, "Potato");
        assert_eq!(food.normalized_name, "potato");
        assert_eq!(food.category_id, Some(vegetables));
        assert_eq!(food.category_source, "llm");
        assert_eq!(food.category_confidence, Some(0.9));

        let by_id = get_food_by_id(&pool, food.id).await.expect("get by id");
        assert_eq!(by_id.as_ref().map(|f| f.id), Some(food.id));

        let by_name = get_food_by_name(&pool, "POTATO  ")
            .await
            .expect("get by name")
            .expect("food found");
        assert_eq!(by_name.id, food.id);

        // Creating the same food again resolves to the same row.
        let again = create_food(&pool, "potato", None, "unknown", None)
            .await
            .expect("create again");
        assert_eq!(again.id, food.id);
        assert_eq!(again.category_id, Some(vegetables), "existing data kept");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM foods")
            .fetch_one(&pool)
            .await
            .expect("count foods");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn concurrent_create_food_is_race_safe() {
        let pool = test_pool().await;

        let a = {
            let pool = pool.clone();
            tokio::spawn(async move { create_food(&pool, "potato", None, "llm", None).await })
        };
        let b = {
            let pool = pool.clone();
            tokio::spawn(async move { create_food(&pool, "Potato ", None, "unknown", None).await })
        };
        let a = a.await.expect("join a").expect("create a");
        let b = b.await.expect("join b").expect("create b");

        assert_eq!(a.id, b.id);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM foods")
            .fetch_one(&pool)
            .await
            .expect("count foods");
        assert_eq!(count, 1);
    }

    /* ---------- alias confirmation rules ---------- */

    #[tokio::test]
    async fn create_alias_confirmation_rules() {
        let pool = test_pool().await;
        let potato = create_food(&pool, "potato", None, "unknown", None)
            .await
            .expect("potato");
        let yam = create_food(&pool, "yam", None, "unknown", None)
            .await
            .expect("yam");

        // First automatic mapping sticks.
        let a1 = create_alias(&pool, "spuds", potato.id, "automatic", false, Some(0.5))
            .await
            .expect("first alias");
        assert_eq!(a1.food_id, potato.id);
        assert!(!a1.confirmed);

        // A later unconfirmed mapping does not replace it.
        let a2 = create_alias(&pool, "Spuds", yam.id, "automatic", false, None)
            .await
            .expect("second alias");
        assert_eq!(a2.food_id, potato.id);
        assert!(!a2.confirmed);
        assert_eq!(a2.source, "automatic");

        // A confirmed mapping replaces an unconfirmed non-user mapping.
        let a3 = create_alias(&pool, "spuds", yam.id, "llm", true, None)
            .await
            .expect("confirmed alias");
        assert_eq!(a3.food_id, yam.id);
        assert!(a3.confirmed);
        assert_eq!(a3.source, "llm");

        // User confirmation always wins and can redirect an existing mapping.
        let a4 = confirm_alias(&pool, "spuds", potato.id)
            .await
            .expect("user confirm");
        assert_eq!(a4.food_id, potato.id);
        assert_eq!(a4.source, "user");
        assert!(a4.confirmed);

        // Even a confirmed automatic mapping cannot touch a user mapping.
        let a5 = create_alias(&pool, "spuds", yam.id, "llm", true, None)
            .await
            .expect("automatic confirmed alias");
        assert_eq!(a5.food_id, potato.id);
        assert_eq!(a5.source, "user");
        assert!(a5.confirmed);

        // A later user confirmation is the latest user choice.
        let a6 = confirm_alias(&pool, "spuds", yam.id)
            .await
            .expect("re-confirm");
        assert_eq!(a6.food_id, yam.id);
        assert_eq!(a6.source, "user");
        assert!(a6.confirmed);

        // find_alias sees the final state.
        let found = find_alias(&pool, "SPUDS")
            .await
            .expect("find alias")
            .expect("alias found");
        assert_eq!(found.food_id, yam.id);
        assert!(found.confirmed);
        assert!(
            find_alias(&pool, "nope")
                .await
                .expect("find miss")
                .is_none()
        );
    }

    /* ---------- category provenance ---------- */

    #[tokio::test]
    async fn set_food_category_respects_user_provenance() {
        let pool = test_pool().await;
        let food = create_food(&pool, "potato", None, "unknown", None)
            .await
            .expect("create food");
        let vegetables = category_id(&pool, "Vegetables").await;
        let pantry = category_id(&pool, "Pantry").await;

        assert!(
            set_food_category(&pool, food.id, Some(vegetables), "llm", Some(0.8), false)
                .await
                .expect("llm set")
        );

        assert!(
            set_food_category(&pool, food.id, Some(pantry), "user", None, true)
                .await
                .expect("user set")
        );

        // Automatic set is refused on user-owned categories.
        assert!(
            !set_food_category(&pool, food.id, Some(vegetables), "llm", Some(0.9), false)
                .await
                .expect("guarded set")
        );

        let food = get_food_by_id(&pool, food.id)
            .await
            .expect("reload")
            .expect("food exists");
        assert_eq!(food.category_id, Some(pantry));
        assert_eq!(food.category_source, "user");
        assert_eq!(food.category_confidence, None);
    }

    #[tokio::test]
    async fn catalog_validates_references() {
        let pool = test_pool().await;

        assert!(create_food(&pool, "   ", None, "llm", None).await.is_err());
        assert!(
            create_alias(&pool, "   ", 1, "automatic", false, None)
                .await
                .is_err()
        );
        assert!(confirm_alias(&pool, "   ", 1).await.is_err());

        assert!(
            create_alias(&pool, "spuds", 999, "automatic", false, None)
                .await
                .is_err()
        );
        assert!(confirm_alias(&pool, "spuds", 999).await.is_err());

        let food = create_food(&pool, "potato", None, "unknown", None)
            .await
            .expect("create food");
        assert!(
            set_food_category(&pool, food.id, Some(4242), "llm", None, false)
                .await
                .is_err()
        );
        assert!(
            set_food_category(&pool, 999, None, "llm", None, false)
                .await
                .is_err()
        );

        assert!(
            get_food_by_id(&pool, 999)
                .await
                .expect("miss by id")
                .is_none()
        );
        assert!(
            get_food_by_name(&pool, "missing")
                .await
                .expect("miss by name")
                .is_none()
        );
    }

    /* ---------- search ---------- */

    #[tokio::test]
    async fn search_foods_matches_names_and_aliases() {
        let pool = test_pool().await;
        let vegetables = category_id(&pool, "Vegetables").await;

        let potato = create_food(&pool, "potato", Some(vegetables), "llm", None)
            .await
            .expect("potato");
        let sweet = create_food(&pool, "sweet potato", Some(vegetables), "llm", None)
            .await
            .expect("sweet potato");
        let starch = create_food(&pool, "potato starch", None, "unknown", None)
            .await
            .expect("potato starch");
        let cilantro = create_food(&pool, "cilantro", None, "unknown", None)
            .await
            .expect("cilantro");
        create_food(&pool, "milk", None, "unknown", None)
            .await
            .expect("milk");

        create_alias(&pool, "spuds", potato.id, "automatic", false, None)
            .await
            .expect("alias spuds");
        confirm_alias(&pool, "chinese parsley", cilantro.id)
            .await
            .expect("alias chinese parsley");

        let hits = search_foods(&pool, "pot", 10).await.expect("search pot");
        let ids: Vec<i64> = hits.iter().map(|h| h.id).collect();
        // Exact/prefix name hits first (alphabetical), then contains.
        assert_eq!(ids, vec![potato.id, starch.id, sweet.id]);
        assert_eq!(hits[0].category.as_deref(), Some("Vegetables"));
        assert!(hits[0].matched_alias.is_none());

        let hits = search_foods(&pool, "spud", 10).await.expect("search spud");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, potato.id);
        assert_eq!(hits[0].matched_alias.as_deref(), Some("spuds"));

        let hits = search_foods(&pool, "parsley", 10)
            .await
            .expect("search parsley");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, cilantro.id);

        let hits = search_foods(&pool, "sweet potato", 10)
            .await
            .expect("search exact");
        assert_eq!(hits[0].id, sweet.id);

        // Limit and empty/escaped queries behave.
        assert_eq!(search_foods(&pool, "pot", 1).await.expect("limit").len(), 1);
        assert!(
            search_foods(&pool, "", 10)
                .await
                .expect("empty query")
                .is_empty()
        );
        assert!(
            search_foods(&pool, "%", 10)
                .await
                .expect("escaped query")
                .is_empty()
        );
    }

    /* ---------- legacy seeding (migration from string aliases) ---------- */
    #[tokio::test]
    async fn migration_adds_food_columns_to_shopping_items_and_view() {
        let pool = test_pool().await;

        let potato = seed_food(&pool, "potato").await;
        let vegetables = category_id(&pool, "Vegetables").await;
        sqlx::query("UPDATE foods SET category_id = ? WHERE id = ?")
            .bind(vegetables)
            .bind(potato)
            .execute(&pool)
            .await
            .expect("set food category");

        // Row 1: no override -> category comes from the Food.
        let (plain_id,): (i64,) = sqlx::query_as(
            "INSERT INTO shopping_items (name, unit, quantity, done, key, food_id) \
             VALUES ('potato', 'g', 500.0, 0, 'g|potato', ?) RETURNING id",
        )
        .bind(potato)
        .fetch_one(&pool)
        .await
        .expect("insert plain item");

        // Row 2: one-time override wins over the Food's category.
        let (overridden_id,): (i64,) = sqlx::query_as(
            "INSERT INTO shopping_items (name, unit, quantity, done, key, food_id, category_override_id) \
             VALUES ('potato', 'g', 1.0, 0, 'g2|potato', ?, ?) RETURNING id",
        )
        .bind(potato)
        .bind(category_id(&pool, "Pantry").await)
        .fetch_one(&pool)
        .await
        .expect("insert overridden item");

        let (plain_food, plain_name, plain_qty, plain_unit, plain_cat, plain_over): (
            Option<i64>,
            String,
            Option<f64>,
            Option<String>,
            Option<i64>,
            bool,
        ) = sqlx::query_as(
            "SELECT food_id, name, quantity, unit, category_id, category_is_override \
               FROM shopping_items_view WHERE id = ?",
        )
        .bind(plain_id)
        .fetch_one(&pool)
        .await
        .expect("plain view row");
        assert_eq!(plain_food, Some(potato));
        assert_eq!(plain_name, "potato");
        assert_eq!(plain_qty, Some(500.0));
        assert_eq!(plain_unit, Some("g".to_string()));
        assert_eq!(plain_cat, Some(vegetables));
        assert!(!plain_over);

        let (over_cat, over_flag): (Option<i64>, bool) = sqlx::query_as(
            "SELECT category_id, category_is_override FROM shopping_items_view WHERE id = ?",
        )
        .bind(overridden_id)
        .fetch_one(&pool)
        .await
        .expect("overridden view row");
        assert_eq!(over_cat, Some(category_id(&pool, "Pantry").await));
        assert!(over_flag);
    }
}
