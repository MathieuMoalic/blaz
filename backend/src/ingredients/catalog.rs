//! Food catalog persistence: canonical foods, aliases, and categories.

use std::collections::{BTreeMap, HashMap};

use sqlx::SqlitePool;

use crate::units::normalize_name;

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

/// Upsert one legacy alias.
///
/// A *confirmed* legacy alias replaces an existing unconfirmed non-user
/// mapping; any other conflict leaves the existing row untouched so neither
/// user-confirmed nor newer automatic mappings are silently rewritten.
async fn upsert_legacy_alias(
    pool: &SqlitePool,
    alias: &str,
    normalized_alias: &str,
    food_id: i64,
    confirmed: bool,
) -> Result<u64, sqlx::Error> {
    const SQL: &str = r"
        INSERT INTO food_aliases (alias, normalized_alias, food_id, source, confirmed)
        VALUES (?, ?, ?, 'migrated', ?)
        ON CONFLICT(normalized_alias) DO UPDATE SET
            alias = CASE
                WHEN excluded.confirmed = 1
                     AND food_aliases.confirmed = 0
                     AND food_aliases.source <> 'user'
                THEN excluded.alias
                ELSE food_aliases.alias
            END,
            food_id = CASE
                WHEN excluded.confirmed = 1
                     AND food_aliases.confirmed = 0
                     AND food_aliases.source <> 'user'
                THEN excluded.food_id
                ELSE food_aliases.food_id
            END,
            confirmed = CASE
                WHEN excluded.confirmed = 1
                     AND food_aliases.confirmed = 0
                     AND food_aliases.source <> 'user'
                THEN 1
                ELSE food_aliases.confirmed
            END
        ";

    let res = sqlx::query(SQL)
        .bind(alias)
        .bind(normalized_alias)
        .bind(food_id)
        .bind(confirmed)
        .execute(pool)
        .await?;

    Ok(res.rows_affected())
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

        let res = sqlx::query(
            r"
            INSERT INTO foods (canonical_name, normalized_name, category_id, category_source)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(normalized_name) DO NOTHING
            ",
        )
        .bind(canonical_name)
        .bind(&normalized)
        .bind(category_id)
        .bind(category_source)
        .execute(pool)
        .await?;
        foods_created += res.rows_affected();

        let (food_id,): (i64,) = sqlx::query_as("SELECT id FROM foods WHERE normalized_name = ?")
            .bind(&normalized)
            .fetch_one(pool)
            .await?;

        for row in rows {
            let normalized_alias = normalize_name(&row.raw_name);
            if normalized_alias.is_empty() {
                tracing::warn!(
                    raw = %row.raw_name,
                    "legacy alias normalizes to empty, skipping"
                );
                continue;
            }

            aliases_written += upsert_legacy_alias(
                pool,
                &row.raw_name,
                &normalized_alias,
                food_id,
                row.confirmed,
            )
            .await?;
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
