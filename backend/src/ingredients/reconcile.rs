//! Deterministic catalog reconciliation.
//!
//! One idempotent maintenance pass over an existing Food catalog:
//!
//! 1. **Curated consolidation** — merge explicitly approved qualifier-
//!    duplicate Foods (e.g. Ground Cumin → Cumin). Never fuzzy: the pair
//!    list is a small, reviewed constant.
//! 2. **Shadow alias reconciliation** — automatic aliases whose text is
//!    another Food's canonical name are removed (canonical lookup owns the
//!    exact name). User-confirmed aliases are never touched.
//! 3. **Legacy garbage cleanup** — instruction/CoT-dump Foods seeded by
//!    migration 00020 are resolved by name: high-confidence references are
//!    re-pointed to real Foods, everything else drops back to
//!    needs-review. No LLM.
//!
//! Every phase is safe to run repeatedly.

use std::collections::HashMap;

use sqlx::SqlitePool;

use crate::ingredients::catalog;
use crate::models::Ingredient;
use crate::units::normalize_name;

/// Curated merge list: `(keep, merge_away)` by normalized canonical name.
///
/// Reviewed by hand; deliberately tiny. Variety/color/drying distinctions
/// (`red onion`, `dried thyme`, `green bell pepper`, ...) must NOT be added
/// without an explicit product decision.
const CURATED_MERGES: &[(&str, &str)] = &[("cumin", "ground cumin")];

/// Summary of one reconciliation pass.
#[derive(Default, Debug)]
pub struct ReconcileReport {
    pub foods_merged: Vec<(String, String)>,
    pub aliases_deleted: Vec<String>,
    pub aliases_repointed: Vec<String>,
    pub garbage_foods_deleted: Vec<String>,
    pub refs_repointed: usize,
    pub refs_cleared: usize,
    pub shopping_rows_merged: usize,
}

/// Run all reconciliation phases.
///
/// # Errors
///
/// Returns an error when a database access fails.
pub async fn run(pool: &SqlitePool) -> anyhow::Result<ReconcileReport> {
    let mut report = ReconcileReport::default();
    consolidate_curated(pool, &mut report).await?;
    reconcile_shadow_aliases(pool, &mut report).await?;
    cleanup_legacy_garbage(pool, &mut report).await?;
    Ok(report)
}

/* =========================
 * Phase 1: curated consolidation
 * ========================= */

async fn consolidate_curated(
    pool: &SqlitePool,
    report: &mut ReconcileReport,
) -> anyhow::Result<()> {
    for (keep_name, away_name) in CURATED_MERGES {
        let Some(keep) = catalog::get_food_by_name(pool, keep_name).await? else {
            continue;
        };
        let Some(away) = catalog::get_food_by_name(pool, away_name).await? else {
            continue;
        };
        if keep.id == away.id {
            continue;
        }

        // Preserve category provenance sensibly: a user-locked category on
        // the duplicate wins over anything the survivor has; otherwise the
        // survivor's own category stands, and an uncategorized survivor
        // inherits the duplicate's category.
        if away.category_source == "user" && keep.category_source != "user" {
            catalog::set_food_category(pool, keep.id, away.category_id, "user", None, true).await?;
        } else if keep.category_id.is_none()
            && keep.category_source != "user"
            && away.category_id.is_some()
        {
            catalog::set_food_category(
                pool,
                keep.id,
                away.category_id,
                &away.category_source,
                away.category_confidence,
                false,
            )
            .await?;
        }

        // Capture the duplicate's aliases, re-point recipe/shopping
        // references, and remove the duplicate BEFORE creating the alias
        // rows on the survivor: while the duplicate Food still exists, its
        // own canonical name would (correctly) block shadow-alias creation.
        let away_aliases: Vec<(String,)> =
            sqlx::query_as("SELECT alias FROM food_aliases WHERE food_id = ?")
                .bind(away.id)
                .fetch_all(pool)
                .await?;
        sqlx::query("DELETE FROM food_aliases WHERE food_id = ?")
            .bind(away.id)
            .execute(pool)
            .await?;

        // Re-point recipe ingredient food_ids.
        repoint_recipe_refs(
            pool,
            away.id,
            keep.id,
            &mut report.refs_repointed,
            &mut report.refs_cleared,
        )
        .await?;

        // Re-point shopping rows, then merge the collisions the merge
        // creates (same Food + compatible unit).
        sqlx::query("UPDATE shopping_items SET food_id = ? WHERE food_id = ?")
            .bind(keep.id)
            .bind(away.id)
            .execute(pool)
            .await?;
        let merged = merge_colliding_rows(pool, keep.id).await?;
        report.shopping_rows_merged += merged;

        // The duplicate is now unreferenced: remove it.
        sqlx::query("DELETE FROM foods WHERE id = ?")
            .bind(away.id)
            .execute(pool)
            .await?;

        // Preserve the duplicate's useful aliases on the survivor.
        for (alias,) in &away_aliases {
            if let Ok(row) =
                catalog::create_alias(pool, alias, keep.id, "automatic", false, None).await
                && row.food_id == keep.id
            {
                report.aliases_repointed.push(alias.clone());
            }
        }
        report
            .foods_merged
            .push((keep.canonical_name.clone(), away.canonical_name.clone()));
    }
    Ok(())
}

/// Merge shopping rows of one Food that collide on the same storage key
/// (same Food + same storage unit → the app's merge identity).
async fn merge_colliding_rows(pool: &SqlitePool, food_id: i64) -> anyhow::Result<usize> {
    let rows: Vec<(i64, Option<f64>, Option<String>, String)> = sqlx::query_as(
        "SELECT id, quantity, unit, COALESCE(recipe_ids, '[]') \
         FROM shopping_items WHERE food_id = ? ORDER BY id",
    )
    .bind(food_id)
    .fetch_all(pool)
    .await?;

    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, (_, _, unit, _)) in rows.iter().enumerate() {
        let key = format!("f:{food_id}|{}", unit.clone().unwrap_or_default());
        groups.entry(key).or_default().push(i);
    }

    let mut merged = 0usize;
    for (group_key, indexes) in groups {
        if indexes.len() <= 1 {
            continue;
        }
        let survivor_idx = indexes[0];
        let survivor_id = rows[survivor_idx].0;
        // Delete duplicates BEFORE rewriting the survivor's key: the new
        // canonical key may collide with a duplicate's existing key.
        for &i in &indexes[1..] {
            let (dup_id, ..) = rows[i];
            // Move contribution history to the survivor.
            sqlx::query(
                "UPDATE shopping_item_sources SET shopping_item_id = ? WHERE shopping_item_id = ?",
            )
            .bind(survivor_id)
            .bind(dup_id)
            .execute(pool)
            .await?;
            sqlx::query("DELETE FROM shopping_items WHERE id = ?")
                .bind(dup_id)
                .execute(pool)
                .await?;
        }
        // Sum quantities and merge recipe id lists into the survivor.
        let mut total: Option<f64> = Some(0.0);
        for &i in &indexes {
            total = match (total, rows[i].1) {
                (Some(a), Some(b)) => Some(a + b),
                _ => None,
            };
        }
        let mut recipe_ids: Vec<i64> = Vec::new();
        for &i in &indexes {
            if let Ok(values) = serde_json::from_str::<Vec<i64>>(&rows[i].3) {
                for v in values {
                    if !recipe_ids.contains(&v) {
                        recipe_ids.push(v);
                    }
                }
            }
        }
        let rids_json = serde_json::to_string(&recipe_ids)?;
        sqlx::query("UPDATE shopping_items SET quantity = ?, recipe_ids = ?, key = ? WHERE id = ?")
            .bind(total)
            .bind(&rids_json)
            .bind(&group_key)
            .bind(survivor_id)
            .execute(pool)
            .await?;
        merged += indexes.len() - 1;
    }
    Ok(merged)
}

/* =========================
 * Phase 2: shadow aliases
 * ========================= */

async fn reconcile_shadow_aliases(
    pool: &SqlitePool,
    report: &mut ReconcileReport,
) -> anyhow::Result<()> {
    // Automatic (non-confirmed, non-user) aliases whose text is another
    // Food's canonical name are removed: the canonical Food owns its exact
    // name, and canonical lookup is authoritative. User-confirmed shadows
    // are documented, never silently changed.
    let deleted: Vec<(String,)> = sqlx::query_as(
        "SELECT a.alias FROM food_aliases a \
         JOIN foods f ON f.normalized_name = a.normalized_alias \
         WHERE a.food_id <> f.id \
           AND a.source <> 'user' AND a.confirmed = 0",
    )
    .fetch_all(pool)
    .await?;
    if !deleted.is_empty() {
        sqlx::query(
            "DELETE FROM food_aliases \
             WHERE source <> 'user' AND confirmed = 0 AND normalized_alias IN ( \
               SELECT a.normalized_alias FROM food_aliases a \
               JOIN foods f ON f.normalized_name = a.normalized_alias \
               WHERE a.food_id <> f.id AND a.source <> 'user' AND a.confirmed = 0)",
        )
        .execute(pool)
        .await?;
        report
            .aliases_deleted
            .extend(deleted.into_iter().map(|(a,)| a));
    }
    Ok(())
}

/* =========================
 * Phase 3: legacy garbage cleanup
 * ========================= */

/// Find a confident replacement Food for a legacy phrase (exact name, exact
/// alias, or leading-qualifier strip). Lookup only — never creates Foods.
async fn find_replacement(pool: &SqlitePool, phrase: &str) -> anyhow::Result<Option<i64>> {
    let normalized = normalize_name(phrase);
    if normalized.is_empty() {
        return Ok(None);
    }
    if let Some(id) = sqlx::query_scalar("SELECT id FROM foods WHERE normalized_name = ?")
        .bind(&normalized)
        .fetch_optional(pool)
        .await?
    {
        return Ok(Some(id));
    }
    if let Some(id) =
        sqlx::query_scalar("SELECT food_id FROM food_aliases WHERE normalized_alias = ?")
            .bind(&normalized)
            .fetch_optional(pool)
            .await?
    {
        return Ok(Some(id));
    }
    // Leading qualifier strip ("medium sweet potato" → "sweet potato").
    if let Some((first, rest)) = normalized.split_once(' ')
        && matches!(first, "small" | "medium" | "large" | "ground")
    {
        let rest = rest.trim();
        let mut candidates = vec![rest.to_string()];
        if let Some(singular) = crate::ingredients::resolver::singularized_phrase(rest) {
            candidates.push(singular);
        }
        for candidate in candidates {
            if let Some(id) = sqlx::query_scalar("SELECT id FROM foods WHERE normalized_name = ?")
                .bind(&candidate)
                .fetch_optional(pool)
                .await?
            {
                return Ok(Some(id));
            }
            if let Some(id) =
                sqlx::query_scalar("SELECT food_id FROM food_aliases WHERE normalized_alias = ?")
                    .bind(&candidate)
                    .fetch_optional(pool)
                    .await?
            {
                return Ok(Some(id));
            }
        }
    }
    Ok(None)
}

async fn cleanup_legacy_garbage(
    pool: &SqlitePool,
    report: &mut ReconcileReport,
) -> anyhow::Result<()> {
    let garbage: Vec<(i64, String)> = sqlx::query_as("SELECT id, canonical_name FROM foods")
        .fetch_all(pool)
        .await?;
    let garbage: Vec<(i64, String)> = garbage
        .into_iter()
        .filter(|(_, name)| {
            catalog::is_pathological_food_name(name, &normalize_name(name)).is_some()
        })
        .collect();

    for (food_id, canonical_name) in garbage {
        // Re-point the garbage Food's aliases to real Foods where the alias
        // text (or its qualifier-stripped form) matches an existing Food.
        let aliases: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT id, alias, normalized_alias FROM food_aliases WHERE food_id = ?",
        )
        .bind(food_id)
        .fetch_all(pool)
        .await?;
        for (alias_id, alias, normalized_alias) in aliases {
            if let Some(target) = find_replacement(pool, &alias).await?
                && target != food_id
            {
                sqlx::query("UPDATE food_aliases SET food_id = ? WHERE id = ?")
                    .bind(target)
                    .bind(alias_id)
                    .execute(pool)
                    .await?;
                report.aliases_repointed.push(alias);
                continue;
            }
            // Unrescuable alias (instruction text, junk): drop it.
            sqlx::query("DELETE FROM food_aliases WHERE id = ?")
                .bind(alias_id)
                .execute(pool)
                .await?;
            report.aliases_deleted.push(normalized_alias);
        }

        // Re-point or clear recipe references.
        repoint_or_clear_recipe_refs(pool, food_id, report).await?;

        // Re-point or clear shopping references.
        repoint_or_clear_shopping_refs(pool, food_id, report).await?;

        sqlx::query("DELETE FROM foods WHERE id = ?")
            .bind(food_id)
            .execute(pool)
            .await?;
        report.garbage_foods_deleted.push(canonical_name);
    }
    Ok(())
}

/// Re-point or clear shopping references to one garbage Food (same
/// deterministic rule as the app's own unresolved handling so a future
/// backfill stays LLM-free).
async fn repoint_or_clear_shopping_refs(
    pool: &SqlitePool,
    food_id: i64,
    report: &mut ReconcileReport,
) -> anyhow::Result<()> {
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, name FROM shopping_items WHERE food_id = ?")
            .bind(food_id)
            .fetch_all(pool)
            .await?;
    for (row_id, name) in rows {
        match find_replacement(pool, &name).await? {
            Some(target) if target != food_id => {
                sqlx::query("UPDATE shopping_items SET food_id = ? WHERE id = ?")
                    .bind(target)
                    .bind(row_id)
                    .execute(pool)
                    .await?;
                report.refs_repointed += 1;
            }
            _ => {
                sqlx::query(
                    "UPDATE shopping_items \
                     SET food_id = NULL, resolution_source = 'unresolved' WHERE id = ?",
                )
                .bind(row_id)
                .execute(pool)
                .await?;
                report.refs_cleared += 1;
            }
        }
    }
    Ok(())
}

/// Re-point or clear recipe references to one garbage Food, based on the
/// ingredient phrase (same deterministic rule as `find_replacement`).
async fn repoint_or_clear_recipe_refs(
    pool: &SqlitePool,
    food_id: i64,
    report: &mut ReconcileReport,
) -> anyhow::Result<()> {
    let recipes: Vec<(i64, String)> = sqlx::query_as("SELECT id, ingredients FROM recipes")
        .fetch_all(pool)
        .await?;
    for (recipe_id, json) in recipes {
        let mut ings: Vec<Ingredient> = serde_json::from_str(&json).unwrap_or_default();
        let mut changed = false;
        for ing in &mut ings {
            if ing.food_id != Some(food_id) {
                continue;
            }
            match find_replacement(pool, &ing.name).await? {
                Some(target) if target != food_id => {
                    ing.food_id = Some(target);
                    ing.needs_review = false;
                    ing.resolution_source = Some("deterministic".to_string());
                    report.refs_repointed += 1;
                }
                _ => {
                    ing.food_id = None;
                    ing.needs_review = true;
                    ing.resolution_source = Some("unresolved".to_string());
                    report.refs_cleared += 1;
                }
            }
            changed = true;
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
    Ok(())
}

/// Re-point every recipe ingredient referencing `from` to `to`.
async fn repoint_recipe_refs(
    pool: &SqlitePool,
    from: i64,
    to: i64,
    repointed: &mut usize,
    _cleared: &mut usize,
) -> anyhow::Result<()> {
    let recipes: Vec<(i64, String)> = sqlx::query_as("SELECT id, ingredients FROM recipes")
        .fetch_all(pool)
        .await?;
    for (recipe_id, json) in recipes {
        if !json.contains(&format!("\"food_id\":{from}"))
            && !json.contains(&format!("\"food_id\": {from}"))
        {
            continue;
        }
        let mut ings: Vec<Ingredient> = serde_json::from_str(&json).unwrap_or_default();
        let mut changed = false;
        for ing in &mut ings {
            if ing.food_id == Some(from) {
                ing.food_id = Some(to);
                changed = true;
                *repointed += 1;
            }
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

    async fn seed_food(pool: &SqlitePool, name: &str, category_id: Option<i64>) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO foods (canonical_name, normalized_name, category_id, category_source) \
             VALUES (?, ?, ?, 'llm') RETURNING id",
        )
        .bind(name)
        .bind(normalize_name(name))
        .bind(category_id)
        .fetch_one(pool)
        .await
        .expect("seed food")
    }

    async fn seed_alias(pool: &SqlitePool, alias: &str, food_id: i64, confirmed: bool) {
        sqlx::query(
            "INSERT INTO food_aliases (alias, normalized_alias, food_id, source, confirmed) \
             VALUES (?, ?, ?, 'automatic', ?)",
        )
        .bind(alias)
        .bind(normalize_name(alias))
        .bind(food_id)
        .bind(confirmed)
        .execute(pool)
        .await
        .expect("seed alias");
    }

    async fn seed_shopping(
        pool: &SqlitePool,
        name: &str,
        food_id: i64,
        unit: &str,
        qty: f64,
    ) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO shopping_items (name, unit, quantity, done, key, food_id) \
             VALUES (?, ?, ?, 0, ?, ?) RETURNING id",
        )
        .bind(name)
        .bind(unit)
        .bind(qty)
        .bind(format!("f:{food_id}|{unit}"))
        .bind(food_id)
        .fetch_one(pool)
        .await
        .expect("seed shopping")
    }

    /* ---------- curated consolidation ---------- */

    #[tokio::test]
    async fn consolidates_curated_pair_and_merges_shopping_rows() {
        let pool = test_pool().await;
        let (pantry,): (i64,) =
            sqlx::query_as("SELECT id FROM shopping_categories WHERE name='Pantry'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let cumin = seed_food(&pool, "cumin", None).await;
        let ground = seed_food(&pool, "ground cumin", Some(pantry)).await;
        seed_alias(&pool, "tbs cumin", cumin, false).await;
        seed_alias(&pool, "ground cumin", ground, false).await;

        // Recipe referencing the duplicate.
        sqlx::query("INSERT INTO recipes (title, ingredients, instructions) VALUES ('R', ?, '[]')")
            .bind(
                serde_json::to_string(&[serde_json::json!({
                    "name": "1 tsp ground cumin",
                    "food_id": ground
                })])
                .unwrap(),
            )
            .execute(&pool)
            .await
            .unwrap();

        // Two shopping rows that collide (tsp) plus one distinct unit row.
        let a = seed_shopping(&pool, "ground cumin", ground, "tsp", 0.5).await;
        let b = seed_shopping(&pool, "cumin", cumin, "tsp", 1.0).await;
        let c = seed_shopping(&pool, "ground cumin", ground, "g", 20.0).await;

        let report = run(&pool).await.unwrap();

        // Merge recorded; duplicate gone.
        assert_eq!(
            report.foods_merged,
            vec![("cumin".to_string(), "ground cumin".to_string())]
        );
        let ground_left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM foods WHERE id = ?")
            .bind(ground)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(ground_left, 0);

        // Alias preserved and re-pointed to the survivor.
        let (alias_food,): (i64,) = sqlx::query_as(
            "SELECT food_id FROM food_aliases WHERE normalized_alias='ground cumin'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(alias_food, cumin);

        // Recipe ref re-pointed.
        let (fid,): (i64,) = sqlx::query_as(
            "SELECT json_extract(value,'$.food_id') FROM recipes, json_each(recipes.ingredients) \
             WHERE title='R'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(fid, cumin);

        // Colliding tsp rows merged to 1.5 tsp; the g row remains separate.
        let rows: Vec<(i64, Option<f64>, Option<String>)> = sqlx::query_as(
            "SELECT id, quantity, unit FROM shopping_items WHERE food_id = ? ORDER BY id",
        )
        .bind(cumin)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 2);
        let tsp = rows.iter().find(|(id, _, _)| *id == a || *id == b).unwrap();
        assert_eq!(tsp.1, Some(1.5));
        assert_eq!(tsp.2.as_deref(), Some("tsp"));
        let g = rows.iter().find(|(id, _, _)| *id == c).unwrap();
        assert_eq!(g.1, Some(20.0));

        // Survivor inherited the duplicate's LLM category.
        let (cat, src): (i64, String) =
            sqlx::query_as("SELECT category_id, category_source FROM foods WHERE id = ?")
                .bind(cumin)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cat, pantry);
        assert_eq!(src, "llm");
    }

    #[tokio::test]
    async fn consolidation_is_idempotent() {
        let pool = test_pool().await;
        let cumin = seed_food(&pool, "cumin", None).await;
        let _ground = seed_food(&pool, "ground cumin", None).await;
        run(&pool).await.unwrap();
        let first: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM foods")
            .fetch_one(&pool)
            .await
            .unwrap();
        run(&pool).await.unwrap();
        let second: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM foods")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(first, 1);
        assert_eq!(second, 1);
        assert!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM foods WHERE id = ?")
                .bind(cumin)
                .fetch_one(&pool)
                .await
                .unwrap()
                == 1
        );
    }

    /* ---------- shadow aliases ---------- */

    #[tokio::test]
    async fn automatic_shadow_aliases_are_removed_user_kept() {
        let pool = test_pool().await;
        let spinach = seed_food(&pool, "spinach", None).await;
        let _baby = seed_food(&pool, "baby spinach", None).await;
        let wine = seed_food(&pool, "white wine", None).await;

        // Automatic shadow: alias text == another food's canonical name.
        seed_alias(&pool, "baby spinach", spinach, false).await;
        // User-confirmed shadow: never touched.
        sqlx::query(
            "INSERT INTO food_aliases (alias, normalized_alias, food_id, source, confirmed) \
             VALUES ('white wine', 'white wine', ?, 'user', 1)",
        )
        .bind(spinach)
        .execute(&pool)
        .await
        .unwrap();
        let _ = wine;

        let report = run(&pool).await.unwrap();

        // Automatic shadow removed; user shadow kept.
        assert!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM food_aliases WHERE normalized_alias='baby spinach'"
            )
            .fetch_one(&pool)
            .await
            .unwrap()
                == 0
        );
        let kept: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM food_aliases WHERE normalized_alias='white wine' AND source='user'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(kept, 1);
        assert_eq!(report.aliases_deleted.len(), 1);
        assert_eq!(report.aliases_deleted[0], "baby spinach");
    }

    /* ---------- legacy garbage ---------- */

    #[tokio::test]
    async fn legacy_garbage_food_is_repaired_and_removed() {
        let pool = test_pool().await;
        let kale = seed_food(&pool, "kale", None).await;
        let sweet = seed_food(&pool, "sweet potato", None).await;

        // Representative migration-00020 garbage: CoT dump owning the plain
        // phrase, and a modifier-only Food holding sweet-potato wording.
        let garbage = sqlx::query_scalar(
            "INSERT INTO foods (canonical_name, normalized_name, category_source) \
             VALUES ('here''s the normalized output for \"kale\": ' || char(10) || \
                     '```json' || char(10) || '\"kale\"' || char(10) || '```', \
                     'here s the normalized output for kale json kale', 'migrated') \
             RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let sweet_food = sqlx::query_scalar(
            "INSERT INTO foods (canonical_name, normalized_name, category_source) \
             VALUES ('sweet', 'sweet', 'migrated') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        seed_alias(&pool, "kale", garbage, false).await;
        seed_alias(&pool, "serve with", garbage, false).await;
        seed_alias(&pool, "medium sweet potato", sweet_food, false).await;

        sqlx::query("INSERT INTO recipes (title, ingredients, instructions) VALUES ('G', ?, '[]')")
            .bind(
                serde_json::to_string(&[
                    serde_json::json!({"name": "kale", "food_id": garbage}),
                    serde_json::json!({"name": "medium sweet potatoes", "food_id": sweet_food}),
                ])
                .unwrap(),
            )
            .execute(&pool)
            .await
            .unwrap();

        let report = run(&pool).await.unwrap();

        // Both garbage Foods removed.
        assert_eq!(report.garbage_foods_deleted.len(), 2);

        // The plain phrase is owned by the canonical Food: either the alias
        // was re-pointed or it was removed as a shadow — resolution is via
        // canonical lookup either way.
        let kale_alias: Option<i64> =
            sqlx::query_scalar("SELECT food_id FROM food_aliases WHERE normalized_alias='kale'")
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert!(kale_alias.is_none_or(|fid| fid == kale));
        assert!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM food_aliases WHERE normalized_alias='serve with'"
            )
            .fetch_one(&pool)
            .await
            .unwrap()
                == 0
        );

        // References repaired: kale re-pointed, sweet-potato wording
        // re-pointed via qualifier strip.
        let refs: Vec<(String, Option<i64>, i64)> = sqlx::query_as(
            "SELECT json_extract(value,'$.name'), json_extract(value,'$.food_id'), \
                    json_extract(value,'$.needs_review') \
             FROM recipes, json_each(recipes.ingredients) WHERE title='G'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(refs[0].0, "kale");
        assert_eq!(refs[0].1, Some(kale));
        assert_eq!(refs[0].2, 0);
        assert_eq!(refs[1].1, Some(sweet));
        assert_eq!(report.refs_repointed, 2);
    }
}
