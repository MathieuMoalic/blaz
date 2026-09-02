//! Backfill command tests: parser-first preparation, idempotency, and
//! stable Food reuse.

use sqlx::SqlitePool;

use crate::config::Config;
use crate::ingredients::backfill;
use crate::models::Ingredient;

fn test_config(database_path: String) -> Config {
    Config {
        verbose: 0,
        quiet: 0,
        bind: "127.0.0.1:0".parse().unwrap(),
        media_dir: std::path::PathBuf::from("/tmp"),
        database_path,
        log_file: std::path::PathBuf::from("/tmp/backfill-test.log"),
        cors_origin: None,
        jwt_secret: Some("test".to_string()),
        password_hash: None,
        llm_api_key: None, // deterministic-only backfill in tests
        llm_api_url: "http://localhost/".to_string(),
        llm_model: None,
        llm_fallback_model: None,
        system_prompt_import: String::new(),
        system_prompt_extract: String::new(),
        system_prompt_structure: String::new(),
        system_prompt_convert: String::new(),
        system_prompt_macros: String::new(),
        system_prompt_normalize: String::new(),
        system_prompt_food_resolver: String::new(),
        system_prompt_prep_reminders: String::new(),
        ntfy_url: None,
    }
}

fn ingredient(name: &str) -> Ingredient {
    Ingredient {
        section: None,
        quantity: None,
        unit: None,
        name: name.to_string(),
        prep: None,
        ingredient_id: None,
        raw_text: None,
        food_id: None,
        qualifiers: Vec::new(),
        resolution_source: None,
        resolution_confidence: None,
        needs_review: false,
        raw: false,
        canonical_name: None,
    }
}

async fn setup_database(pool: &SqlitePool) {
    // Known food + alias (deterministic resolution, id 1 on a fresh DB).
    sqlx::query(
        "INSERT INTO foods (canonical_name, normalized_name, category_id, category_source) \
         VALUES ('potato', 'potato', 3, 'migrated')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO food_aliases (alias, normalized_alias, food_id, source, confirmed) \
         VALUES ('potatoes', 'potatoes', 1, 'automatic', 1)",
    )
    .execute(pool)
    .await
    .unwrap();

    // Recipe: one unresolved raw line + one legacy structured line.
    sqlx::query("INSERT INTO recipes (title, ingredients, instructions) VALUES ('Mash', ?, '[]')")
        .bind(r#"[{"name":"2 potatoes","raw":true},{"quantity":1.0,"unit":"kg","name":"onion"}]"#)
        .execute(pool)
        .await
        .unwrap();

    // Legacy shopping row without food identity.
    sqlx::query(
        "INSERT INTO shopping_items (name, unit, quantity, done, key) \
         VALUES ('potatoes', NULL, 5.0, 0, '|potatoes')",
    )
    .execute(pool)
    .await
    .unwrap();
}

/* ---------- parser-first preparation (§2, §3, §4) ---------- */

type LegacyCase = (
    &'static str,
    &'static str,
    Option<f64>,
    Option<&'static str>,
    Option<&'static str>,
    Option<&'static str>,
);

#[test]
fn prepare_parses_legacy_lines_into_phrases() {
    let cases: &[LegacyCase] = &[
        // (original name, resolver phrase, qty, unit, prep, raw_text)
        (
            "1/2 cup peanuts, chopped ($0.24)",
            "peanuts",
            Some(0.5),
            Some("cup"),
            Some("chopped"),
            Some("1/2 cup peanuts, chopped ($0.24)"),
        ),
        (
            "1/2 tsp dried basil, chopped",
            "dried basil",
            Some(0.5),
            Some("tsp"),
            Some("chopped"),
            Some("1/2 tsp dried basil, chopped"),
        ),
        (
            "1/2 cup mushroom soaking liquid",
            "mushroom soaking liquid",
            Some(0.5),
            Some("cup"),
            None,
            Some("1/2 cup mushroom soaking liquid"),
        ),
        (
            "3 large potatoes, peeled",
            "large potatoes",
            Some(3.0),
            None,
            Some("peeled"),
            Some("3 large potatoes, peeled"),
        ),
        (
            "1 1/2 teaspoons ground cumin",
            "ground cumin",
            Some(1.5),
            Some("tsp"),
            None,
            Some("1 1/2 teaspoons ground cumin"),
        ),
    ];

    for (line, phrase, qty, unit, prep, raw_text) in cases {
        let mut ing = ingredient(line);
        let got = backfill::prepare_ingredient_for_resolution(&mut ing);

        // Quantity/unit/preparation never appear in the resolver input.
        assert_eq!(got, *phrase, "resolver input for {line:?}");
        assert_eq!(ing.quantity, *qty, "qty for {line:?}");
        assert_eq!(ing.unit.as_deref(), *unit, "unit for {line:?}");
        assert_eq!(ing.prep.as_deref(), *prep, "prep for {line:?}");
        assert_eq!(ing.name, *phrase, "cleaned recipe name for {line:?}");
        assert_eq!(ing.raw_text.as_deref(), *raw_text, "raw_text for {line:?}");
        assert!(!ing.raw, "raw flag cleared for {line:?}");
    }
}

#[test]
fn prepare_is_conservative_for_structured_ingredients() {
    // qty/unit already structured, name is a plain phrase: untouched.
    let mut ing = ingredient("olive oil");
    ing.quantity = Some(1.0);
    ing.unit = Some("tbsp".to_string());
    let phrase = backfill::prepare_ingredient_for_resolution(&mut ing);
    assert_eq!(phrase, "olive oil");
    assert_eq!(ing.name, "olive oil");
    assert_eq!(ing.quantity, Some(1.0));
    assert_eq!(ing.unit.as_deref(), Some("tbsp"));
    assert!(
        ing.raw_text.is_none(),
        "structured rows aren't given raw_text"
    );

    // Plain legacy names with no embedded structure are left intact.
    let mut ing = ingredient("salt");
    let phrase = backfill::prepare_ingredient_for_resolution(&mut ing);
    assert_eq!(phrase, "salt");
    assert_eq!(ing.name, "salt");
    assert!(ing.raw_text.is_none());

    // Trailing "to taste" becomes prep, not resolver input.
    let mut ing = ingredient("salt to taste");
    let phrase = backfill::prepare_ingredient_for_resolution(&mut ing);
    assert_eq!(phrase, "salt");
    assert_eq!(ing.name, "salt");
    assert_eq!(ing.prep.as_deref(), Some("to taste"));
    assert_eq!(ing.raw_text.as_deref(), Some("salt to taste"));
}

/* ---------- idempotency (§1) ---------- */

#[test]
fn backfill_is_idempotent_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test.sqlite").to_string_lossy().to_string();
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let (first, second, pool) = runtime.block_on(async {
        let pool = crate::db::make_pool(db_path.clone()).await.unwrap();
        setup_database(&pool).await;
        let config = test_config(db_path.clone());
        let first = backfill::run(&pool, &config, false).await.unwrap();
        let second = backfill::run(&pool, &config, false).await.unwrap();
        (first, second, pool)
    });

    // First run: recipe ingredients + shopping row resolved.
    assert_eq!(first.ingredients_updated, 2);
    assert_eq!(first.shopping_updated, 1);
    assert_eq!(first.foods_created, 0, "potato already exists");

    // Second default run: nothing rewritten, no new foods/aliases, and the
    // still-unresolved "onion" is skipped (attempted, no LLM retry).
    assert_eq!(second.ingredients_updated, 0, "no re-write");
    assert_eq!(second.shopping_updated, 0);
    assert_eq!(second.foods_created, 0);
    assert_eq!(second.aliases_created, 0, "no new aliases");
    assert!(
        second.skipped_attempted >= 1,
        "unresolved entries are skipped"
    );

    let food_ids: Vec<Option<i64>> = runtime.block_on(async {
        sqlx::query_scalar("SELECT food_id FROM shopping_items")
            .fetch_all(&pool)
            .await
            .unwrap()
    });
    assert_eq!(food_ids, vec![Some(1)]);

    let json_json: String = runtime.block_on(async {
        sqlx::query_scalar("SELECT ingredients FROM recipes")
            .fetch_one(&pool)
            .await
            .unwrap()
    });
    let ings: Vec<Ingredient> = serde_json::from_str(&json_json).unwrap();
    assert_eq!(ings[0].food_id, Some(1));
    assert!(ings[0].ingredient_id.is_some());
    // Parser-first: the raw line is structured; wording stored in raw_text.
    assert_eq!(ings[0].name, "potatoes");
    assert_eq!(ings[0].quantity, Some(2.0));
    assert_eq!(ings[0].raw_text.as_deref(), Some("2 potatoes"));
    assert_eq!(ings[1].food_id, None, "onion stays unresolved without LLM");
    assert!(ings[1].needs_review);
    assert!(ings[1].ingredient_id.is_some());

    let food_count: i64 = runtime.block_on(async {
        sqlx::query_scalar("SELECT COUNT(*) FROM foods")
            .fetch_one(&pool)
            .await
            .unwrap()
    });
    let alias_count: i64 = runtime.block_on(async {
        sqlx::query_scalar("SELECT COUNT(*) FROM food_aliases")
            .fetch_one(&pool)
            .await
            .unwrap()
    });
    assert_eq!(food_count, 1);
    assert_eq!(alias_count, 1, "the seeded alias is reused, no new ones");
}

#[test]
fn retry_unresolved_reprocesses_attempted_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test.sqlite").to_string_lossy().to_string();
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let (second, pool) = runtime.block_on(async {
        let pool = crate::db::make_pool(db_path.clone()).await.unwrap();
        setup_database(&pool).await;
        let config = test_config(db_path.clone());
        let _ = backfill::run(&pool, &config, false).await.unwrap();
        let second = backfill::run(&pool, &config, true).await.unwrap();
        (second, pool)
    });

    // The already-resolved potato is still reused (skipped), while the
    // unresolved onion is re-attempted once more (still unresolved without
    // an LLM, so it stays flagged but processed).
    assert_eq!(second.skipped_attempted, 1, "potato is reused");
    assert_eq!(second.ingredients_updated, 1, "onion is re-attempted");
    let _ = pool;
}

/* ---------- stable Food reuse (§5) ---------- */

#[test]
fn existing_normalized_food_is_reused_not_duplicated() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test.sqlite").to_string_lossy().to_string();
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let food_id = runtime.block_on(async {
        let pool = crate::db::make_pool(db_path.clone()).await.unwrap();
        crate::db::MIGRATOR.run(&pool).await.unwrap();

        // A Food already exists (whatever spelling it was created under).
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO foods (canonical_name, normalized_name) \
             VALUES ('red cabbage', 'red cabbage') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // A legacy recipe line for the same normalized Food.
        sqlx::query(
            "INSERT INTO recipes (title, ingredients, instructions) VALUES ('Salad', ?, '[]')",
        )
        .bind(r#"[{"name":"red cabbage"}]"#)
        .execute(&pool)
        .await
        .unwrap();

        let config = test_config(db_path.clone());
        let _ = backfill::run(&pool, &config, false).await.unwrap();

        // Exactly one Food red cabbage survives; the recipe references it.
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM foods")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1, "no duplicate Food created");

        let ing_json: String = sqlx::query_scalar("SELECT ingredients FROM recipes")
            .fetch_one(&pool)
            .await
            .unwrap();
        let ings: Vec<Ingredient> = serde_json::from_str(&ing_json).unwrap();
        assert_eq!(ings[0].food_id, Some(id), "reused the existing Food");
        id
    });

    let _ = food_id;
}
