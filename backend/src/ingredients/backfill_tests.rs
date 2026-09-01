//! Backfill command tests: idempotency and legacy → Food identity.

use sqlx::SqlitePool;

use crate::config::Config;
use crate::ingredients::backfill;

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

    // Recipe with one unresolved raw line and one legacy structured line.
    sqlx::query(
        "INSERT INTO recipes (title, ingredients, instructions) VALUES ('Mash', ?, '[]')",
    )
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

#[test]
fn backfill_is_idempotent_and_backfills_food_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test.sqlite").to_string_lossy().to_string();
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let (first, second, pool) = runtime.block_on(async {
        let pool = crate::db::make_pool(db_path.clone()).await.unwrap();
        setup_database(&pool).await;
        let config = test_config(db_path.clone());
        let first = backfill::run(&pool, &config).await.unwrap();
        let second = backfill::run(&pool, &config).await.unwrap();
        (first, second, pool)
    });

    // First run: recipe ingredient + shopping row resolved.
    assert_eq!(first.ingredients_updated, 2);
    assert_eq!(first.shopping_updated, 1);
    assert_eq!(first.foods_created, 0, "potato already exists");

    // Idempotent: a second run writes nothing new.
    assert_eq!(second.ingredients_updated, 0, "no re-write");
    assert_eq!(second.shopping_updated, 0);
    assert_eq!(second.foods_created, 0, "no new foods");

    let food_ids: Vec<Option<i64>> =
        runtime.block_on(async { sqlx::query_scalar("SELECT food_id FROM shopping_items").fetch_all(&pool).await.unwrap() });
    assert_eq!(food_ids, vec![Some(1)]);

    let json_json: String = runtime.block_on(async {
        sqlx::query_scalar("SELECT ingredients FROM recipes")
            .fetch_one(&pool)
            .await
            .unwrap()
    });
    let ings: Vec<crate::models::Ingredient> = serde_json::from_str(&json_json).unwrap();
    assert_eq!(ings[0].food_id, Some(1));
    assert!(ings[0].ingredient_id.is_some());
    assert_eq!(ings[0].name, "2 potatoes", "recipe wording preserved");
    assert_eq!(ings[1].food_id, None, "onion stays unresolved without LLM");
    assert!(ings[1].needs_review);
    assert!(ings[1].ingredient_id.is_some());

    let food_count: i64 = runtime.block_on(async {
        sqlx::query_scalar("SELECT COUNT(*) FROM foods").fetch_one(&pool).await.unwrap()
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