//! Integration tests for the HTTP API.
//!
//! Each test spins up an in-memory `SQLite` database and uses
//! `tower::ServiceExt::oneshot` to send requests directly to the Axum router
//! — no real network port is bound.

#[cfg(test)]
mod integration {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    // ── test helpers ─────────────────────────────────────────────────────────

    async fn make_test_state(tmp: &tempfile::TempDir) -> crate::models::AppState {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        crate::db::MIGRATOR.run(&pool).await.expect("migrations");

        let jwt_secret = "integration-test-secret".to_string();
        let jwt_encoding = jsonwebtoken::EncodingKey::from_secret(jwt_secret.as_bytes());

        let config = crate::config::Config {
            verbose: 0,
            quiet: 0,
            bind: "127.0.0.1:0".parse().unwrap(),
            media_dir: tmp.path().to_path_buf(),
            database_path: ":memory:".to_string(),
            log_file: tmp.path().join("test.log"),
            cors_origin: None,
            jwt_secret: Some(jwt_secret),
            password_hash: None,
            llm_api_key: None,
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
        };

        crate::models::AppState {
            pool,
            jwt_encoding,
            config,
        }
    }

    fn make_token() -> String {
        use jsonwebtoken::{Algorithm, Header, encode};
        #[derive(serde::Serialize)]
        struct Claims {
            sub: i64,
            exp: u64,
        }

        let exp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;

        encode(
            &Header::new(Algorithm::HS256),
            &Claims { sub: 1, exp },
            &jsonwebtoken::EncodingKey::from_secret(b"integration-test-secret"),
        )
        .unwrap()
    }

    async fn json_body(body: Body) -> Value {
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn auth_get(uri: &str, token: &str) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    }

    fn auth_json(method: &str, uri: &str, token: &str, body: &Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    // ── public endpoints ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn healthz_returns_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let app = crate::app::build_app(make_test_state(&tmp).await);

        let resp = app
            .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn version_returns_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let app = crate::app::build_app(make_test_state(&tmp).await);

        let resp = app
            .oneshot(Request::get("/version").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp.into_body()).await;
        assert!(body["version"].is_string());
    }

    // ── auth guard ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn recipes_list_allows_unauthenticated() {
        let tmp = tempfile::tempdir().unwrap();
        let app = crate::app::build_app(make_test_state(&tmp).await);

        let resp = app
            .oneshot(Request::get("/recipes").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn recipes_list_allows_bad_token() {
        let tmp = tempfile::tempdir().unwrap();
        let app = crate::app::build_app(make_test_state(&tmp).await);

        let resp = app
            .oneshot(
                Request::get("/recipes")
                    .header(header::AUTHORIZATION, "Bearer notavalidtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ── recipe CRUD ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn recipes_list_empty_on_fresh_db() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let token = make_token();
        let app = crate::app::build_app(state);

        let resp = app.oneshot(auth_get("/recipes", &token)).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp.into_body()).await;
        assert_eq!(body.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn recipe_create_and_get() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let token = make_token();
        let app = crate::app::build_app(state);

        let new_recipe = json!({
            "title": "Carbonara",
            "source": "https://example.com/carbonara",
            "yield": "2 servings",
            "notes": "Classic Roman pasta",
            "ingredients": [
                {"quantity": 200.0, "unit": "g", "name": "spaghetti", "raw": false}
            ],
            "instructions": ["Boil pasta", "Mix eggs and cheese", "Combine"]
        });

        let resp = app
            .clone()
            .oneshot(auth_json("POST", "/recipes", &token, &new_recipe))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let created = json_body(resp.into_body()).await;
        let id = created["id"].as_i64().expect("id in response");
        assert_eq!(created["title"], "Carbonara");
        assert_eq!(created["instructions"].as_array().unwrap().len(), 3);

        // GET the individual recipe
        let resp = app
            .oneshot(auth_get(&format!("/recipes/{id}"), &token))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let fetched = json_body(resp.into_body()).await;
        assert_eq!(fetched["id"], id);
        assert_eq!(fetched["title"], "Carbonara");
    }

    #[tokio::test]
    async fn recipe_create_then_list() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let token = make_token();
        let app = crate::app::build_app(state);

        let recipe = json!({"title": "Risotto", "ingredients": [], "instructions": []});
        app.clone()
            .oneshot(auth_json("POST", "/recipes", &token, &recipe))
            .await
            .unwrap();

        let resp = app.oneshot(auth_get("/recipes", &token)).await.unwrap();

        let list = json_body(resp.into_body()).await;
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(list[0]["title"], "Risotto");
    }

    #[tokio::test]
    async fn recipe_update() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let token = make_token();
        let app = crate::app::build_app(state);

        // Create
        let resp = app
            .clone()
            .oneshot(auth_json(
                "POST",
                "/recipes",
                &token,
                &json!({"title": "Old Title", "ingredients": [], "instructions": []}),
            ))
            .await
            .unwrap();
        let id = json_body(resp.into_body()).await["id"].as_i64().unwrap();

        // Update title
        let resp = app
            .clone()
            .oneshot(auth_json(
                "PATCH",
                &format!("/recipes/{id}"),
                &token,
                &json!({"title": "New Title"}),
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let updated = json_body(resp.into_body()).await;
        assert_eq!(updated["title"], "New Title");
    }

    #[tokio::test]
    async fn recipe_delete() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let token = make_token();
        let app = crate::app::build_app(state);

        // Create
        let resp = app
            .clone()
            .oneshot(auth_json(
                "POST",
                "/recipes",
                &token,
                &json!({"title": "Delete Me", "ingredients": [], "instructions": []}),
            ))
            .await
            .unwrap();
        let id = json_body(resp.into_body()).await["id"].as_i64().unwrap();

        // Delete
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/recipes/{id}"))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let resp = app
            .oneshot(auth_get(&format!("/recipes/{id}"), &token))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn recipe_get_nonexistent_returns_404() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let token = make_token();
        let app = crate::app::build_app(state);

        let resp = app
            .oneshot(auth_get("/recipes/999999", &token))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── recipesage import ────────────────────────────────────────────────────

    #[tokio::test]
    async fn recipesage_import_creates_recipes() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let token = make_token();
        let app = crate::app::build_app(state);

        let payload = json!([
            {
                "name": "Imported Soup",
                "url": "https://example.com/soup",
                "recipeIngredient": ["2 carrots", "1 L water"],
                "recipeInstructions": ["Boil everything"],
                "recipeYield": "4"
            },
            {
                "name": "Imported Cake",
                "recipeIngredient": ["200 g flour"],
                "recipeInstructions": [{"@type": "HowToStep", "text": "Mix and bake"}]
            }
        ]);

        let resp = app
            .clone()
            .oneshot(auth_json(
                "POST",
                "/recipes/import/recipesage",
                &token,
                &payload,
            ))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp.into_body()).await;
        assert_eq!(body["imported_count"], 2);
        assert_eq!(body["failed"].as_array().unwrap().len(), 0);

        // Verify recipes actually exist
        let list_resp = app.oneshot(auth_get("/recipes", &token)).await.unwrap();
        let recipes = json_body(list_resp.into_body()).await;
        assert_eq!(recipes.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn recipesage_import_skips_duplicate_by_source() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let token = make_token();
        let app = crate::app::build_app(state);

        let payload = json!([{
            "name": "Pasta",
            "url": "https://example.com/pasta",
            "recipeIngredient": [],
            "recipeInstructions": []
        }]);

        // First import
        app.clone()
            .oneshot(auth_json(
                "POST",
                "/recipes/import/recipesage",
                &token,
                &payload,
            ))
            .await
            .unwrap();

        // Second import — should be skipped
        let resp = app
            .clone()
            .oneshot(auth_json(
                "POST",
                "/recipes/import/recipesage",
                &token,
                &payload,
            ))
            .await
            .unwrap();

        let body = json_body(resp.into_body()).await;
        // The handler returns Ok(()) for both created and skipped, so failed must be empty
        assert_eq!(body["failed"].as_array().unwrap().len(), 0);

        // DB should still have only 1 recipe
        let list_resp = app.oneshot(auth_get("/recipes", &token)).await.unwrap();
        let recipes = json_body(list_resp.into_body()).await;
        assert_eq!(recipes.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn recipesage_import_skips_duplicate_by_title_when_no_source() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let token = make_token();
        let app = crate::app::build_app(state);

        let payload = json!([{
            "name": "No Source Recipe",
            "recipeIngredient": [],
            "recipeInstructions": []
        }]);

        app.clone()
            .oneshot(auth_json(
                "POST",
                "/recipes/import/recipesage",
                &token,
                &payload,
            ))
            .await
            .unwrap();

        app.clone()
            .oneshot(auth_json(
                "POST",
                "/recipes/import/recipesage",
                &token,
                &payload,
            ))
            .await
            .unwrap();

        let list_resp = app.oneshot(auth_get("/recipes", &token)).await.unwrap();
        let recipes = json_body(list_resp.into_body()).await;
        assert_eq!(recipes.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn recipesage_import_invalid_json_returns_400() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let token = make_token();
        let app = crate::app::build_app(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/recipes/import/recipesage")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("not json at all"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // ── shopping list ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn recipe_with_resolution_fields_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let token = make_token();
        let app = crate::app::build_app(state);

        let new_recipe = json!({
            "title": "Curry",
            "ingredients": [
                {
                    "quantity": 3.0, "unit": null, "name": "large potatoes",
                    "prep": "peeled", "ingredient_id": "uuid-a",
                    "raw_text": "3 large potatoes, peeled",
                    "food_id": 42, "qualifiers": ["large"],
                    "resolution_source": "alias", "needs_review": false, "raw": false
                }
            ],
            "instructions": ["Cook"]
        });

        let resp = app
            .clone()
            .oneshot(auth_json("POST", "/recipes", &token, &new_recipe))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let created = json_body(resp.into_body()).await;
        let id = created["id"].as_i64().unwrap();

        let resp = app
            .oneshot(auth_get(&format!("/recipes/{id}"), &token))
            .await
            .unwrap();
        let fetched = json_body(resp.into_body()).await;
        let ing = &fetched["ingredients"][0];
        assert_eq!(ing["ingredient_id"], "uuid-a");
        assert_eq!(ing["raw_text"], "3 large potatoes, peeled");
        assert_eq!(ing["food_id"], 42);
        assert_eq!(ing["qualifiers"], json!(["large"]));
        assert_eq!(ing["resolution_source"], "alias");
        assert_eq!(ing["needs_review"], false);
        // Recipe-visible wording is preserved untouched.
        assert_eq!(ing["name"], "large potatoes");
        assert_eq!(ing["prep"], "peeled");
    }

    #[tokio::test]
    async fn deterministic_structuring_survives_llm_outage() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let llm = crate::llm::LlmClient::new(
            state.config.llm_api_url.clone(),
            String::new(),
            "test-model".to_string(),
        );
        let http = reqwest::Client::new();
        let settings = crate::routes::settings::LlmSettings::default();
        let lines: Vec<String> = [
            "## Sauce",
            "½ kg potatoes",
            "2 cups flour",
            "salt to taste",
            "",
        ]
        .iter()
        .map(std::string::ToString::to_string)
        .collect();

        let ings = crate::routes::parse_recipe::structure_ingredients(
            &llm, &http, &state, &settings, &lines,
        )
        .await;

        assert_eq!(ings[0].section.as_deref(), Some("Sauce"));
        assert_eq!(ings[1].quantity, Some(0.5));
        assert_eq!(ings[1].unit.as_deref(), Some("kg"));
        assert_eq!(ings[1].name, "potatoes");
        assert_eq!(ings[1].raw_text.as_deref(), Some("½ kg potatoes"));
        assert!(ings[1].ingredient_id.is_some());
        assert!(!ings[1].raw);

        // "2 cups flour" needs the LLM, which is unavailable: it falls back
        // to the deterministic parse instead of failing the import.
        assert_eq!(ings[2].quantity, Some(2.0));
        assert_eq!(ings[2].unit, None);
        assert_eq!(ings[2].name, "cups flour");
        assert_eq!(ings[2].raw_text.as_deref(), Some("2 cups flour"));

        assert_eq!(ings[3].name, "salt");
        assert_eq!(ings[3].prep.as_deref(), Some("to taste"));
    }

    async fn seed_food_alias(pool: &sqlx::SqlitePool, canonical: &str, alias: &str) -> i64 {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO foods (canonical_name, normalized_name) \
             VALUES (?, ?) RETURNING id",
        )
        .bind(canonical)
        .bind(canonical)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO food_aliases (alias, normalized_alias, food_id, source, confirmed) \
             VALUES (?, ?, ?, 'automatic', 1)",
        )
        .bind(alias)
        .bind(alias)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
        id
    }

    #[tokio::test]
    async fn recipe_create_structures_and_resolves_raw_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let pool = &state.pool;
        let potato = seed_food_alias(pool, "potato", "potatoes").await;
        let salt = seed_food_alias(pool, "salt", "salt").await;

        let token = make_token();
        let app = crate::app::build_app(state);

        let new_recipe = json!({
            "title": "Mash",
            "ingredients": [
                {"name": "2 potatoes", "raw": true},
                {"quantity": 1.0, "unit": "tsp", "name": "Salt", "raw": false}
            ],
            "instructions": ["Mash"]
        });

        let resp = app
            .clone()
            .oneshot(auth_json("POST", "/recipes", &token, &new_recipe))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let created = json_body(resp.into_body()).await;
        let id = created["id"].as_i64().unwrap();

        let resp = app
            .oneshot(auth_get(&format!("/recipes/{id}"), &token))
            .await
            .unwrap();
        let fetched = json_body(resp.into_body()).await;

        // Raw line was structured server-side and resolved via the alias.
        let ing0 = &fetched["ingredients"][0];
        assert_eq!(ing0["raw"], false);
        assert_eq!(ing0["quantity"], 2.0);
        assert_eq!(ing0["name"], "potatoes", "recipe wording preserved");
        assert_eq!(ing0["food_id"], potato);
        assert_eq!(ing0["resolution_source"], "confirmed_alias");
        assert_eq!(ing0["needs_review"], false);
        assert!(ing0["ingredient_id"].is_string());

        // Case differences resolve through normalization.
        let ing1 = &fetched["ingredients"][1];
        assert_eq!(ing1["food_id"], salt);
        assert_eq!(ing1["needs_review"], false);
    }

    #[tokio::test]
    async fn recipe_update_preserves_and_re_resolves_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let pool = &state.pool;
        let potato = seed_food_alias(pool, "potato", "potatoes").await;

        let token = make_token();
        let app = crate::app::build_app(state);

        let created = json_body(
            app.clone()
                .oneshot(auth_json(
                    "POST",
                    "/recipes",
                    &token,
                    &json!({
                        "title": "Mash",
                        "ingredients": [
                            {"name": "2 potatoes", "raw": true}
                        ],
                        "instructions": ["Mash"]
                    }),
                ))
                .await
                .unwrap()
                .into_body(),
        )
        .await;
        let id = created["id"].as_i64().unwrap();
        let ing_id = created["ingredients"][0]["ingredient_id"]
            .as_str()
            .unwrap()
            .to_string();

        // Quantity-only edit: same name, same ingredient_id → identity kept.
        let resp = app
            .clone()
            .oneshot(auth_json(
                "PATCH",
                &format!("/recipes/{id}"),
                &token,
                &json!({"ingredients": [
                    {"quantity": 5.0, "unit": null, "name": "potatoes",
                     "ingredient_id": ing_id, "prep": null, "raw": false}
                ]}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let updated = json_body(resp.into_body()).await;
        assert_eq!(updated["ingredients"][0]["food_id"], potato);
        assert_eq!(updated["ingredients"][0]["quantity"], 5.0);
        assert_eq!(updated["ingredients"][0]["needs_review"], false);

        // Name change to an unknown food: stale identity is cleared and the
        // ingredient is flagged for review (no LLM available to resolve).
        let resp = app
            .clone()
            .oneshot(auth_json(
                "PATCH",
                &format!("/recipes/{id}"),
                &token,
                &json!({"ingredients": [
                    {"quantity": 5.0, "unit": null, "name": "sweet potatoes",
                     "ingredient_id": ing_id, "prep": null, "raw": false}
                ]}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let updated = json_body(resp.into_body()).await;
        assert_eq!(updated["ingredients"][0]["food_id"], serde_json::Value::Null);
        assert_eq!(updated["ingredients"][0]["needs_review"], true);
    }

    #[tokio::test]
    async fn shopping_list_starts_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let token = make_token();
        let app = crate::app::build_app(state);

        let resp = app.oneshot(auth_get("/shopping", &token)).await.unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            json_body(resp.into_body()).await.as_array().unwrap().len(),
            0
        );
    }

    #[tokio::test]
    async fn shopping_add_and_list_item() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let token = make_token();
        let app = crate::app::build_app(state);

        app.clone()
            .oneshot(auth_json(
                "POST",
                "/shopping",
                &token,
                &json!({"text": "2 kg potatoes"}),
            ))
            .await
            .unwrap();

        let resp = app.oneshot(auth_get("/shopping", &token)).await.unwrap();

        let items = json_body(resp.into_body()).await;
        assert_eq!(items.as_array().unwrap().len(), 1);
        assert!(items[0]["text"].as_str().unwrap().contains("potatoes"));
    }

    // ── canonical ingredient resolution ──────────────────────────────────────

    #[tokio::test]
    async fn ingredient_aliases_cache_canonical_names() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let pool = &state.pool;

        // Manually insert an alias (simulating a previous resolution)
        sqlx::query(
            "INSERT INTO ingredient_aliases (raw_name, canonical_name, confirmed, category, confirmed_category) VALUES (?, ?, ?, ?, ?)"
        )
        .bind("potatoes")
        .bind("potato")
        .bind(0)
        .bind("Vegetables")
        .bind(0)
        .execute(pool)
        .await
        .unwrap();

        // Query it back
        let record: Option<(String, String)> = sqlx::query_as(
            "SELECT canonical_name, category FROM ingredient_aliases WHERE raw_name = ?",
        )
        .bind("potatoes")
        .fetch_optional(pool)
        .await
        .unwrap();

        assert!(record.is_some());
        let (canonical, category) = record.unwrap();
        assert_eq!(canonical, "potato");
        assert_eq!(category, "Vegetables");
    }

    #[tokio::test]
    async fn ingredient_aliases_potato_variants_resolve() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let pool = &state.pool;

        // Insert variants - all should resolve to canonical "potato" in "Vegetables"
        let variants = vec!["potato", "potatoes", "large potatoes", "small potato"];
        for variant in &variants {
            sqlx::query(
                "INSERT INTO ingredient_aliases (raw_name, canonical_name, confirmed, category, confirmed_category) VALUES (?, ?, ?, ?, ?)"
            )
            .bind(variant)
            .bind("potato")
            .bind(0)
            .bind("Vegetables")
            .bind(0)
            .execute(pool)
            .await
            .unwrap();
        }

        // Verify all variants map to the same canonical name and category
        for variant in &variants {
            let record: Option<(String, String)> = sqlx::query_as(
                "SELECT canonical_name, category FROM ingredient_aliases WHERE raw_name = ?",
            )
            .bind(variant)
            .fetch_optional(pool)
            .await
            .unwrap();

            let (canonical, category) = record.unwrap();
            assert_eq!(
                canonical, "potato",
                "variant '{variant}' should map to canonical 'potato'"
            );
            assert_eq!(
                category, "Vegetables",
                "variant '{variant}' category should be 'Vegetables'"
            );
        }
    }

    #[tokio::test]
    async fn ingredient_aliases_distinct_ingredients_not_merged() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let pool = &state.pool;

        // Insert pairs that should NOT be merged
        let pairs = vec![
            (
                "sweet potato",
                "sweetpotato",
                "sweet potato",
                "root vegetables",
            ),
            ("coconut milk", "coconutmilk", "coconut", "Drinks"),
            ("brown sugar", "brownsugar", "sugar", "Pantry"),
            ("peanut butter", "peanutbutter", "peanut", "Pantry"),
        ];

        for (raw_name, _, canonical, category) in &pairs {
            sqlx::query(
                "INSERT INTO ingredient_aliases (raw_name, canonical_name, confirmed, category, confirmed_category) VALUES (?, ?, ?, ?, ?)"
            )
            .bind(raw_name)
            .bind(canonical)
            .bind(0)
            .bind(category)
            .bind(0)
            .execute(pool)
            .await
            .unwrap();
        }

        // Verify they are distinct (not merged with base ingredients)
        let record: Option<String> =
            sqlx::query_scalar("SELECT canonical_name FROM ingredient_aliases WHERE raw_name = ?")
                .bind("sweet potato")
                .fetch_optional(pool)
                .await
                .unwrap();
        assert_eq!(record, Some("sweet potato".to_string()));

        let record: Option<String> =
            sqlx::query_scalar("SELECT canonical_name FROM ingredient_aliases WHERE raw_name = ?")
                .bind("coconut milk")
                .fetch_optional(pool)
                .await
                .unwrap();
        assert_eq!(record, Some("coconut".to_string()));
    }

    #[tokio::test]
    async fn ingredient_aliases_valid_categories_only() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let pool = &state.pool;

        // Verify that only valid shopping categories exist
        let categories: Vec<String> =
            sqlx::query_scalar("SELECT name FROM shopping_categories ORDER BY sort_order")
                .fetch_all(pool)
                .await
                .unwrap();

        assert!(!categories.is_empty());
        assert!(categories.contains(&"Other".to_string()));
        assert!(categories.contains(&"Vegetables".to_string()));
        assert!(categories.contains(&"Fruits".to_string()));

        // Insert an alias with valid category
        sqlx::query(
            "INSERT INTO ingredient_aliases (raw_name, canonical_name, confirmed, category, confirmed_category) VALUES (?, ?, ?, ?, ?)"
        )
        .bind("apple")
        .bind("apple")
        .bind(0)
        .bind("Fruits")
        .bind(0)
        .execute(pool)
        .await
        .unwrap();

        let category: String =
            sqlx::query_scalar("SELECT category FROM ingredient_aliases WHERE raw_name = ?")
                .bind("apple")
                .fetch_one(pool)
                .await
                .unwrap();

        assert!(categories.contains(&category));
    }

    #[tokio::test]
    async fn ingredient_aliases_confirmed_flag_preserved() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let pool = &state.pool;

        // Insert with confirmed=1 (user-set, should not auto-change)
        sqlx::query(
            "INSERT INTO ingredient_aliases (raw_name, canonical_name, confirmed, category, confirmed_category) VALUES (?, ?, ?, ?, ?)"
        )
        .bind("tomato")
        .bind("tomato")
        .bind(1)  // confirmed
        .bind("Vegetables")
        .bind(1)  // confirmed_category
        .execute(pool)
        .await
        .unwrap();

        let record: Option<(i32, i32)> = sqlx::query_as(
            "SELECT confirmed, confirmed_category FROM ingredient_aliases WHERE raw_name = ?",
        )
        .bind("tomato")
        .fetch_optional(pool)
        .await
        .unwrap();

        let (confirmed, confirmed_category) = record.unwrap();
        assert_eq!(confirmed, 1);
        assert_eq!(confirmed_category, 1);
    }

    #[tokio::test]
    async fn ingredient_aliases_auto_generated_has_confirmed_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let pool = &state.pool;

        // Insert with confirmed=0 (auto-generated, can change)
        sqlx::query(
            "INSERT INTO ingredient_aliases (raw_name, canonical_name, confirmed, category, confirmed_category) VALUES (?, ?, ?, ?, ?)"
        )
        .bind("banana")
        .bind("banana")
        .bind(0)  // auto-generated
        .bind("Fruits")
        .bind(0)  // auto-generated
        .execute(pool)
        .await
        .unwrap();

        let record: Option<(i32, i32)> = sqlx::query_as(
            "SELECT confirmed, confirmed_category FROM ingredient_aliases WHERE raw_name = ?",
        )
        .bind("banana")
        .fetch_optional(pool)
        .await
        .unwrap();

        let (confirmed, confirmed_category) = record.unwrap();
        assert_eq!(confirmed, 0);
        assert_eq!(confirmed_category, 0);
    }
}
