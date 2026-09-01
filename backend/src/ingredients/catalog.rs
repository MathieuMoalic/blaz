//! Food catalog persistence: canonical foods, aliases, and categories.

use std::collections::{BTreeMap, HashMap, HashSet};

use sqlx::SqlitePool;

use crate::ingredients::types::{
    CatalogAliasRef, CatalogFoodRef, CatalogSnapshot, Food, FoodAlias, FoodSearchRow,
};
use crate::units::normalize_name;

const FOOD_COLS: &str =
    "id, canonical_name, normalized_name, category_id, category_source, category_confidence";
const ALIAS_COLS: &str =
    "id, alias, normalized_alias, food_id, source, confidence, confirmed";

/// A confirmed incoming alias mapping replaces an existing unconfirmed
/// non-user mapping; every other conflict keeps the existing row.
const ALIAS_WINNER: &str =
    "excluded.confirmed = 1 AND food_aliases.confirmed = 0 AND food_aliases.source <> 'user'";

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
/// silently rewritten). Returns the resulting alias row.
///
/// # Errors
///
/// Returns an error when the alias normalizes to an empty string, the food
/// does not exist, or the database write fails.
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
    if get_food_by_id(pool, food_id).await?.is_none() {
        anyhow::bail!("food {food_id} does not exist");
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
    Ok(sqlx::query_as::<_, FoodAlias>(&select)
        .bind(&normalized)
        .fetch_one(pool)
        .await?)
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
pub async fn confirm_alias(pool: &SqlitePool, alias: &str, food_id: i64) -> anyhow::Result<FoodAlias> {
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
        let known: Option<i64> = sqlx::query_scalar("SELECT id FROM shopping_categories WHERE id = ?")
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

    hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then_with(|| a.2.cmp(&b.2)));

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
 * Legacy seeding
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

/// One row of the legacy string-based `ingredient_aliases` table.
#[derive(sqlx::FromRow)]
#[allow(clippy::struct_field_names)] // mirrors the legacy column names
struct LegacyAlias {
    raw_name: String,
    canonical_name: String,
    confirmed: bool,
    category: Option<String>,
    confirmed_category: Option<bool>,
}

/// Category to seed onto a Food, and whether the user confirmed it.
///
/// A user-confirmed category always wins; otherwise the most frequent
/// category among the aliases is used (ties broken alphabetically so
/// seeding is deterministic).
fn preferred_category(rows: &[LegacyAlias]) -> (Option<&str>, bool) {
    if let Some(row) = rows.iter().find(|r| {
        r.confirmed_category.unwrap_or(false)
            && r.category.as_deref().is_some_and(|c| !c.is_empty())
    }) {
        return (row.category.as_deref(), true);
    }

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for row in rows {
        if let Some(category) = row.category.as_deref().filter(|c| !c.is_empty()) {
            *counts.entry(category).or_default() += 1;
        }
    }

    let mut best: Option<(usize, &str)> = None;
    for (name, count) in counts {
        if let Some((best_count, _)) = best
            && count <= best_count
        {
            continue;
        }
        best = Some((count, name));
    }

    best.map_or((None, false), |(_, name)| (Some(name), false))
}

/// Map a legacy category name to a category ID and provenance.
fn resolve_category(
    category: Option<&str>,
    user_confirmed: bool,
    category_ids: &HashMap<String, i64>,
    canonical_name: &str,
) -> (Option<i64>, &'static str) {
    let Some(name) = category else {
        return (None, "unknown");
    };

    category_ids.get(name).map_or_else(
        || {
            tracing::warn!(
                category = %name,
                canonical = %canonical_name,
                "legacy category not found; food starts uncategorized"
            );
            (None, "unknown")
        },
        |id| (Some(*id), if user_confirmed { "user" } else { "migrated" }),
    )
}

/// Seed `foods` / `food_aliases` from the legacy string-based alias system.
///
/// Every distinct legacy canonical name becomes one Food; every legacy raw
/// name becomes an alias pointing at that Food. Legacy `confirmed` flags are
/// preserved, and a user-confirmed legacy category becomes the Food's
/// category with `category_source = 'user'` so automatic resolution never
/// overwrites it later.
///
/// Idempotent: safe to run on every startup. Existing foods keep their
/// category; existing user or confirmed aliases are never rewritten; a
/// confirmed legacy alias only replaces an existing unconfirmed non-user
/// mapping.
///
/// # Errors
///
/// Returns an error if the legacy table cannot be read or the new rows
/// cannot be written.
pub async fn seed_legacy_aliases(pool: &SqlitePool) -> anyhow::Result<()> {
    let legacy: Vec<LegacyAlias> = sqlx::query_as(
        "SELECT raw_name, canonical_name, confirmed, category, confirmed_category
           FROM ingredient_aliases",
    )
    .fetch_all(pool)
    .await?;

    if legacy.is_empty() {
        tracing::debug!("no legacy ingredient aliases to seed");
        return Ok(());
    }

    let categories: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, name FROM shopping_categories")
            .fetch_all(pool)
            .await?;
    let category_ids: HashMap<String, i64> = categories
        .into_iter()
        .map(|(id, name)| (name, id))
        .collect();

    // Group per canonical name so each Food is created exactly once.
    // `BTreeMap` keeps the iteration order deterministic.
    let mut by_canonical: BTreeMap<String, Vec<LegacyAlias>> = BTreeMap::new();
    for row in legacy {
        by_canonical
            .entry(row.canonical_name.clone())
            .or_default()
            .push(row);
    }

    let mut foods_created: u64 = 0;
    let mut aliases_written: u64 = 0;

    for (canonical_name, rows) in &by_canonical {
        let normalized = normalize_name(canonical_name);
        if normalized.is_empty() {
            tracing::warn!(
                canonical = %canonical_name,
                "legacy canonical name normalizes to empty, skipping"
            );
            continue;
        }

        let (category, user_confirmed) = preferred_category(rows);
        let (category_id, category_source) = resolve_category(
            category,
            user_confirmed,
            &category_ids,
            canonical_name,
        );

        let existed = get_food_by_name(pool, canonical_name).await?.is_some();
        let food = create_food(pool, canonical_name, category_id, category_source, None).await?;
        foods_created += u64::from(!existed);

        for row in rows {
            let normalized_alias = normalize_name(&row.raw_name);
            if normalized_alias.is_empty() {
                tracing::warn!(
                    raw = %row.raw_name,
                    "legacy alias normalizes to empty, skipping"
                );
                continue;
            }

            create_alias(pool, &row.raw_name, food.id, "migrated", row.confirmed, None).await?;
            aliases_written += 1;
        }
    }

    tracing::info!(
        foods_created,
        aliases_written,
        "seeded legacy ingredient aliases into foods/food_aliases"
    );

    Ok(())
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

    async fn insert_legacy(
        pool: &SqlitePool,
        raw_name: &str,
        canonical_name: &str,
        confirmed: bool,
        category: &str,
        confirmed_category: bool,
    ) {
        sqlx::query(
            "INSERT INTO ingredient_aliases \
             (raw_name, canonical_name, confirmed, category, confirmed_category) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(raw_name)
        .bind(canonical_name)
        .bind(confirmed)
        .bind(category)
        .bind(confirmed_category)
        .execute(pool)
        .await
        .expect("insert legacy alias");
    }

    async fn seed_food(pool: &SqlitePool, canonical_name: &str) -> i64 {
        let (id,): (i64,) =
            sqlx::query_as("INSERT INTO foods (canonical_name, normalized_name) VALUES (?, ?) RETURNING id")
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

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM foods")
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
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM foods")
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
        assert!(find_alias(&pool, "nope").await.expect("find miss").is_none());
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
        assert!(create_alias(&pool, "   ", 1, "automatic", false, None).await.is_err());
        assert!(confirm_alias(&pool, "   ", 1).await.is_err());

        assert!(create_alias(&pool, "spuds", 999, "automatic", false, None).await.is_err());
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

        assert!(get_food_by_id(&pool, 999).await.expect("miss by id").is_none());
        assert!(
            get_food_by_name(&pool, "missing").await.expect("miss by name").is_none()
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
            search_foods(&pool, "", 10).await.expect("empty query").is_empty()
        );
        assert!(
            search_foods(&pool, "%", 10).await.expect("escaped query").is_empty()
        );
    }

    /* ---------- legacy seeding (migration from string aliases) ---------- */

    #[tokio::test]
    async fn seeds_foods_and_aliases_from_legacy_rows() {
        let pool = test_pool().await;

        insert_legacy(&pool, "potatoes", "potato", false, "Vegetables", false).await;
        insert_legacy(&pool, "potato", "potato", true, "Vegetables", true).await;
        insert_legacy(&pool, "spuds", "potato", false, "Fruits", false).await;
        insert_legacy(&pool, "onions", "onion", false, "Vegetables", false).await;

        seed_legacy_aliases(&pool).await.expect("seed");

        let foods: Vec<(i64, String, Option<i64>, String)> = sqlx::query_as(
            "SELECT id, canonical_name, category_id, category_source \
               FROM foods ORDER BY canonical_name",
        )
        .fetch_all(&pool)
        .await
        .expect("foods");
        assert_eq!(foods.len(), 2);

        let vegetables = category_id(&pool, "Vegetables").await;

        // User-confirmed category wins over the more frequent one.
        let potato = foods
            .iter()
            .find(|(_, name, _, _)| name == "potato")
            .expect("potato food");
        assert_eq!(potato.2, Some(vegetables));
        assert_eq!(potato.3, "user");

        let aliases: Vec<(String, bool, String)> = sqlx::query_as(
            "SELECT normalized_alias, confirmed, source \
               FROM food_aliases ORDER BY normalized_alias",
        )
        .fetch_all(&pool)
        .await
        .expect("aliases");
        assert_eq!(aliases.len(), 4);
        assert_eq!(aliases[0].0, "onions");
        assert_eq!(aliases[1].0, "potato");
        assert!(aliases[1].1);
        assert_eq!(aliases[2].0, "potatoes");
        assert!(!aliases[2].1);
        assert_eq!(aliases[3].0, "spuds");
        for (_, _, source) in &aliases {
            assert_eq!(source, "migrated");
        }

        // All potato variants point at the same Food.
        let distinct_foods: Vec<i64> = sqlx::query_scalar(
            "SELECT DISTINCT food_id FROM food_aliases \
              WHERE normalized_alias IN ('potatoes', 'potato', 'spuds')",
        )
        .fetch_all(&pool)
        .await
        .expect("distinct foods");
        assert_eq!(distinct_foods, vec![potato.0]);
    }

    #[tokio::test]
    async fn seeding_is_idempotent() {
        let pool = test_pool().await;
        insert_legacy(&pool, "potatoes", "potato", false, "Vegetables", false).await;

        seed_legacy_aliases(&pool).await.expect("first run");
        seed_legacy_aliases(&pool).await.expect("second run");

        let food_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM foods")
                .fetch_one(&pool)
                .await
                .expect("count foods");
        assert_eq!(food_count, 1);

        let alias_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM food_aliases")
                .fetch_one(&pool)
                .await
                .expect("count aliases");
        assert_eq!(alias_count, 1);
    }

    #[tokio::test]
    async fn confirmed_legacy_alias_overrides_unconfirmed_automatic() {
        let pool = test_pool().await;

        let yam_id = seed_food(&pool, "yam").await;
        sqlx::query(
            "INSERT INTO food_aliases (alias, normalized_alias, food_id, source, confirmed) \
             VALUES ('sweet potatoes', 'sweet potatoes', ?, 'automatic', 0)",
        )
        .bind(yam_id)
        .execute(&pool)
        .await
        .expect("pre-insert automatic alias");

        // The user had confirmed "sweet potatoes" -> "sweet potato" in the legacy system.
        insert_legacy(&pool, "sweet potatoes", "sweet potato", true, "Vegetables", false).await;

        seed_legacy_aliases(&pool).await.expect("seed");

        let (food_id, confirmed): (i64, bool) = sqlx::query_as(
            "SELECT food_id, confirmed FROM food_aliases WHERE normalized_alias = 'sweet potatoes'",
        )
        .fetch_one(&pool)
        .await
        .expect("alias row");
        let sweet_potato: (i64,) =
            sqlx::query_as("SELECT id FROM foods WHERE normalized_name = 'sweet potato'")
                .fetch_one(&pool)
                .await
                .expect("sweet potato food");

        assert_eq!(food_id, sweet_potato.0);
        assert!(confirmed);
    }

    #[tokio::test]
    async fn user_alias_never_overwritten_by_legacy_data() {
        let pool = test_pool().await;

        let cilantro = seed_food(&pool, "cilantro").await;
        sqlx::query(
            "INSERT INTO food_aliases (alias, normalized_alias, food_id, source, confirmed) \
             VALUES ('chinese parsley', 'chinese parsley', ?, 'user', 1)",
        )
        .bind(cilantro)
        .execute(&pool)
        .await
        .expect("pre-insert user alias");

        insert_legacy(&pool, "chinese parsley", "coriander", true, "Seasoning", false).await;

        seed_legacy_aliases(&pool).await.expect("seed");

        let (food_id, source, confirmed): (i64, String, bool) = sqlx::query_as(
            "SELECT food_id, source, confirmed FROM food_aliases \
              WHERE normalized_alias = 'chinese parsley'",
        )
        .fetch_one(&pool)
        .await
        .expect("alias row");

        assert_eq!(food_id, cilantro);
        assert_eq!(source, "user");
        assert!(confirmed);
    }

    #[tokio::test]
    async fn category_falls_back_to_most_frequent_then_null() {
        let pool = test_pool().await;

        insert_legacy(&pool, "whole milk", "milk", false, "Drinks", false).await;
        insert_legacy(&pool, "milk", "milk", false, "Drinks", false).await;
        insert_legacy(&pool, "oat milk", "oat milk", false, "Nonexistent Category", false).await;

        seed_legacy_aliases(&pool).await.expect("seed");

        let (milk_category, milk_source): (Option<i64>, String) = sqlx::query_as(
            "SELECT category_id, category_source FROM foods WHERE normalized_name = 'milk'",
        )
        .fetch_one(&pool)
        .await
        .expect("milk food");
        let drinks = category_id(&pool, "Drinks").await;
        assert_eq!(milk_category, Some(drinks));
        assert_eq!(milk_source, "migrated");

        let (oat_category, oat_source): (Option<i64>, String) = sqlx::query_as(
            "SELECT category_id, category_source FROM foods WHERE normalized_name = 'oat milk'",
        )
        .fetch_one(&pool)
        .await
        .expect("oat milk food");
        assert_eq!(oat_category, None);
        assert_eq!(oat_source, "unknown");
    }

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

    #[tokio::test]
    async fn seeding_noop_without_legacy_rows() {
        let pool = test_pool().await;
        seed_legacy_aliases(&pool).await.expect("seed");

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM foods")
                .fetch_one(&pool)
                .await
                .expect("count foods");
        assert_eq!(count, 0);
    }
}
