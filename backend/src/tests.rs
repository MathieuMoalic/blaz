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
        crate::db::MIGRATOR
            .run(&pool)
            .await
            .expect("migrations");

        let jwt_secret = "integration-test-secret".to_string();
        let jwt_encoding =
            jsonwebtoken::EncodingKey::from_secret(jwt_secret.as_bytes());

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
            system_prompt_prep_reminders: String::new(),
            ntfy_url: None,
        };

        crate::models::AppState { pool, jwt_encoding, config }
    }

    fn make_token() -> String {
        use jsonwebtoken::{Algorithm, Header, encode};
        #[derive(serde::Serialize)]
        struct Claims { sub: i64, exp: u64 }

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

        let resp = app
            .oneshot(auth_get("/recipes", &token))
            .await
            .unwrap();

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

        let resp = app
            .oneshot(auth_get("/recipes", &token))
            .await
            .unwrap();

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
        let list_resp = app
            .oneshot(auth_get("/recipes", &token))
            .await
            .unwrap();
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
            .oneshot(auth_json("POST", "/recipes/import/recipesage", &token, &payload))
            .await
            .unwrap();

        // Second import — should be skipped
        let resp = app
            .clone()
            .oneshot(auth_json("POST", "/recipes/import/recipesage", &token, &payload))
            .await
            .unwrap();

        let body = json_body(resp.into_body()).await;
        // The handler returns Ok(()) for both created and skipped, so failed must be empty
        assert_eq!(body["failed"].as_array().unwrap().len(), 0);

        // DB should still have only 1 recipe
        let list_resp = app
            .oneshot(auth_get("/recipes", &token))
            .await
            .unwrap();
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
            .oneshot(auth_json("POST", "/recipes/import/recipesage", &token, &payload))
            .await
            .unwrap();

        app.clone()
            .oneshot(auth_json("POST", "/recipes/import/recipesage", &token, &payload))
            .await
            .unwrap();

        let list_resp = app
            .oneshot(auth_get("/recipes", &token))
            .await
            .unwrap();
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
    async fn shopping_list_starts_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let token = make_token();
        let app = crate::app::build_app(state);

        let resp = app
            .oneshot(auth_get("/shopping", &token))
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(json_body(resp.into_body()).await.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn shopping_add_and_list_item() {
        let tmp = tempfile::tempdir().unwrap();
        let state = make_test_state(&tmp).await;
        let token = make_token();
        let app = crate::app::build_app(state);

        app.clone()
            .oneshot(auth_json("POST", "/shopping", &token, &json!({"text": "2 kg potatoes"})))
            .await
            .unwrap();

        let resp = app
            .oneshot(auth_get("/shopping", &token))
            .await
            .unwrap();

        let items = json_body(resp.into_body()).await;
        assert_eq!(items.as_array().unwrap().len(), 1);
        assert!(items[0]["text"].as_str().unwrap().contains("potatoes"));
    }
}
