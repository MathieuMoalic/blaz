use std::{
    net::TcpListener as StdTcpListener,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};

use argon2::Argon2;
use password_hash::{PasswordHasher, SaltString};
use rand::rngs::OsRng;
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::Mutex;

fn generate_password_hash(password: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .unwrap()
        .to_string()
}

fn pick_free_port() -> u16 {
    // Bind to port 0 to let OS pick a free port.
    // We drop it immediately; slight race risk, but good enough for tests.
    let l = StdTcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    l.local_addr().unwrap().port()
}

struct TestServer {
    _tmp: TempDir,
    port: u16,
    child: Child,
}

const TEST_PASSWORD: &str = "testpassword123";

impl TestServer {
    fn start() -> Self {
        Self::start_inner(None)
    }

    fn start_with_ntfy(ntfy_url: &str) -> Self {
        Self::start_inner(Some(ntfy_url))
    }

    fn start_inner(ntfy_url: Option<&str>) -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let port = pick_free_port();

        let db_path = tmp.path().join("test.sqlite");
        let media_dir = tmp.path().join("media");
        let log_file = tmp.path().join("test.log");

        // Ensure dirs exist for cleaner startup
        std::fs::create_dir_all(&media_dir).ok();

        // Generate password hash for this test run
        let password_hash = generate_password_hash(TEST_PASSWORD);

        // NOTE: env!("CARGO_BIN_EXE_blaz") is provided by Cargo for integration tests
        // and points at the compiled binary.
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_blaz"));
        cmd.env("BLAZ_BIND_ADDR", format!("127.0.0.1:{port}"))
            .env("BLAZ_DATABASE_PATH", db_path.to_string_lossy().to_string())
            .env("BLAZ_MEDIA_DIR", media_dir.to_string_lossy().to_string())
            .env("BLAZ_LOG_FILE", log_file.to_string_lossy().to_string())
            .env("BLAZ_PASSWORD_HASH", password_hash)
            .env_remove("BLAZ_LLM_API_KEY") // Don't use LLM in tests (for predictability)
            // keep output quiet unless test fails
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        if let Some(url) = ntfy_url {
            cmd.env("BLAZ_NTFY_URL", url);
        }

        let child = cmd.spawn().expect("spawn blaz");

        Self {
            _tmp: tmp,
            port,
            child,
        }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        // Best effort cleanup
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Minimal HTTP server that captures POST bodies, used to mock an ntfy endpoint.
struct MockNtfy {
    port: u16,
    received: Arc<Mutex<Vec<String>>>,
}

impl MockNtfy {
    async fn start() -> Self {
        use axum::{Router, routing::post};

        let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let store = received.clone();

        let app = Router::new().route(
            "/{*path}",
            post(move |body: String| {
                let store = store.clone();
                async move {
                    store.lock().await.push(body);
                    axum::http::StatusCode::OK
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock ntfy");
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(axum::serve(listener, app).into_future());

        Self { port, received }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/test-topic", self.port)
    }

    /// Poll until `count` messages arrive or `timeout_ms` elapses.
    async fn wait_for_messages(&self, count: usize, timeout_ms: u64) -> Vec<String> {
        let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            let msgs = self.received.lock().await.clone();
            if msgs.len() >= count || std::time::Instant::now() >= deadline {
                return msgs;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

async fn wait_ready(base: &str) {
    let client = reqwest::Client::new();
    let mut waited = Duration::from_millis(0);

    loop {
        match client.get(format!("{base}/healthz")).send().await {
            Ok(resp) if resp.status().is_success() => return,
            _ => {}
        }

        if waited >= Duration::from_secs(3) {
            panic!("server did not become ready (healthz)");
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
        waited += Duration::from_millis(50);
    }
}

async fn login(base: &str) -> String {
    let client = reqwest::Client::new();

    let login = client
        .post(format!("{base}/auth/login"))
        .json(&json!({"password": TEST_PASSWORD}))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    login["token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn healthz_ok() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;

    let text = reqwest::get(format!("{base}/healthz"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(
        text.contains("ok"),
        "expected healthz to contain ok, got: {text}"
    );
}

#[tokio::test]
async fn auth_login_ok() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;

    let client = reqwest::Client::new();

    // Login with correct password => returns token
    let login = client
        .post(format!("{base}/auth/login"))
        .json(&json!({"password": TEST_PASSWORD}))
        .send()
        .await
        .unwrap();

    assert_eq!(login.status(), reqwest::StatusCode::OK);
    let body = login.json::<serde_json::Value>().await.unwrap();
    assert!(
        body.get("token")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .len()
            > 10
    );
}

#[tokio::test]
async fn auth_login_wrong_password() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;

    let client = reqwest::Client::new();

    // Wrong password => 401
    let resp = client
        .post(format!("{base}/auth/login"))
        .json(&json!({"password": "wrongpassword"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn recipes_crud_smoke() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    // Create
    let created = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "title": "Test Pasta",
            "source": "unit-test",
            "yield": "2 servings",
            "notes": "",
            "ingredients": [
                { "quantity": 200.0, "unit": "g",    "name": "pasta" },
                { "quantity":   1.0, "unit": "tbsp", "name": "olive oil" },
                {                          "name": "salt" }
            ],
            "instructions": ["Boil pasta", "Toss with oil", "Salt to taste"]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(created.status(), reqwest::StatusCode::OK);
    let created_js = created.json::<serde_json::Value>().await.unwrap();
    let id = created_js["id"].as_i64().expect("id");

    // Get
    let got = client
        .get(format!("{base}/recipes/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(got.status(), reqwest::StatusCode::OK);

    // List contains it
    let list = client
        .get(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    assert!(
        list.as_array().unwrap().iter().any(|r| r["id"] == id),
        "created recipe not found in list"
    );

    // Patch title
    let upd = client
        .patch(format!("{base}/recipes/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"title":"Test Pasta Updated"}))
        .send()
        .await
        .unwrap();
    assert_eq!(upd.status(), reqwest::StatusCode::OK);

    // Delete
    let del = client
        .delete(format!("{base}/recipes/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), reqwest::StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn shopping_done_and_category_behavior() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    // Create an item
    let created = client
        .post(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"text":"2 apples"}))
        .send()
        .await
        .unwrap();

    assert_eq!(created.status(), reqwest::StatusCode::OK);
    let item_js = created.json::<serde_json::Value>().await.unwrap();
    let id = item_js["id"].as_i64().expect("id");

    // With no LLM API key configured, classifier falls back to "Other"
    assert_eq!(item_js["category"].as_str(), Some("Other"));
    assert_eq!(item_js["done"].as_i64(), Some(0));

    // Mark item as done
    let patched = client
        .patch(format!("{base}/shopping/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({ "done": true }))
        .send()
        .await
        .unwrap();

    assert_eq!(patched.status(), reqwest::StatusCode::OK);
    let patched_js = patched.json::<serde_json::Value>().await.unwrap();
    assert_eq!(patched_js["done"].as_i64(), Some(1));

    // Listing active shopping items should now hide this one
    let list = client
        .get(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    let arr = list.as_array().expect("shopping list must be an array");
    assert!(
        arr.iter().all(|row| row["id"] != id),
        "done item should not appear in /shopping list"
    );
}

#[tokio::test]
async fn shopping_create_and_merge_smoke() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    // Create simple quantity item
    let item = client
        .post(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"text":"2 apples"}))
        .send()
        .await
        .unwrap();

    assert_eq!(item.status(), reqwest::StatusCode::OK);
    let item_js = item.json::<serde_json::Value>().await.unwrap();
    assert!(item_js["text"].as_str().unwrap_or("").contains("apples"));

    // Merge more apples
    let merged = client
        .post(format!("{base}/shopping/merge"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "items": [
                {"quantity": 1, "unit": null, "name": "apples", "category": null}
            ]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(merged.status(), reqwest::StatusCode::OK);
    let list = merged.json::<serde_json::Value>().await.unwrap();
    assert!(
        list.as_array()
            .unwrap()
            .iter()
            .any(|x| { x["text"].as_str().unwrap_or("").contains("apples") })
    );
}

#[tokio::test]
async fn recipes_not_found() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/recipes/99999"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);

    let resp = client
        .patch(format!("{base}/recipes/99999"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"title":"Updated"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);

    let resp = client
        .delete(format!("{base}/recipes/99999"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn shopping_parsing_variations() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"text":"milk"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let resp = client
        .post(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"text":"1.5 L water"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let resp = client
        .post(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"text":"2-3 kg potatoes"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let resp = client
        .post(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"text":"200 g flour"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn shopping_empty_text_rejected() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"text":""}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    let resp = client
        .post(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"text":"   "}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn shopping_category_validation() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let created = client
        .post(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"text":"milk"}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), reqwest::StatusCode::OK);
    let item_js = created.json::<serde_json::Value>().await.unwrap();
    let id = item_js["id"].as_i64().expect("id");

    let resp = client
        .patch(format!("{base}/shopping/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"category":"Fruits"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let resp = client
        .patch(format!("{base}/shopping/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"category":"InvalidCategory"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    let resp = client
        .patch(format!("{base}/shopping/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"category":""}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn shopping_delete() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let created = client
        .post(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"text":"test item"}))
        .send()
        .await
        .unwrap();
    let item_js = created.json::<serde_json::Value>().await.unwrap();
    let id = item_js["id"].as_i64().expect("id");

    let resp = client
        .delete(format!("{base}/shopping/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let list = client
        .get(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    assert!(list.as_array().unwrap().iter().all(|x| x["id"] != id));
}

#[tokio::test]
async fn shopping_merge_accumulates_quantities() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    client
        .post(format!("{base}/shopping/merge"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "items": [
                {"quantity": 100.0, "unit": "g", "name": "flour", "category": null}
            ]
        }))
        .send()
        .await
        .unwrap();

    client
        .post(format!("{base}/shopping/merge"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "items": [
                {"quantity": 200.0, "unit": "g", "name": "flour", "category": null}
            ]
        }))
        .send()
        .await
        .unwrap();

    let list = client
        .get(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    let flour_item = list
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["text"].as_str().unwrap_or("").contains("flour"))
        .expect("flour item should exist");

    assert!(flour_item["text"].as_str().unwrap().contains("300"));
}

#[tokio::test]
async fn meal_plan_crud() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let recipe = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "title": "Test Recipe",
            "source": "test",
            "ingredients": [],
            "instructions": []
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let recipe_id = recipe["id"].as_i64().expect("recipe id");

    let assigned = client
        .post(format!("{base}/meal-plan"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"day":"2026-02-15","recipe_id":recipe_id}))
        .send()
        .await
        .unwrap();
    assert_eq!(assigned.status(), reqwest::StatusCode::OK);

    let day_plan = client
        .get(format!("{base}/meal-plan?day=2026-02-15"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(day_plan.as_array().unwrap().len(), 1);

    let deleted = client
        .delete(format!("{base}/meal-plan/2026-02-15/{recipe_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), reqwest::StatusCode::OK);

    let day_plan = client
        .get(format!("{base}/meal-plan?day=2026-02-15"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(day_plan.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn meal_plan_non_existent_recipe() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/meal-plan"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"day":"2026-02-15","recipe_id":99999}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn meal_plan_duplicate_assignment() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let recipe = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "title": "Test Recipe",
            "source": "test",
            "ingredients": [],
            "instructions": []
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let recipe_id = recipe["id"].as_i64().expect("recipe id");

    client
        .post(format!("{base}/meal-plan"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"day":"2026-02-15","recipe_id":recipe_id}))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{base}/meal-plan"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"day":"2026-02-15","recipe_id":recipe_id}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CONFLICT);
}

#[tokio::test]
async fn recipes_invalid_title() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "title": "",
            "source": "test",
            "ingredients": [],
            "instructions": []
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    let resp = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "title": "   ",
            "source": "test",
            "ingredients": [],
            "instructions": []
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn recipes_with_ingredients() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let created = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "title": "Complex Recipe",
            "source": "test",
            "yield": "4 servings",
            "ingredients": [
                {"quantity": 500.0, "unit": "g", "name": "pasta"},
                {"quantity": 2.0, "unit": "tbsp", "name": "olive oil"},
                {"name": "salt"},
                {"quantity": null, "unit": null, "name": "pepper"}
            ],
            "instructions": ["Step 1", "Step 2", "Step 3"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), reqwest::StatusCode::OK);

    let recipe = created.json::<serde_json::Value>().await.unwrap();
    assert_eq!(recipe["title"], "Complex Recipe");
    assert_eq!(recipe["ingredients"].as_array().unwrap().len(), 4);
    assert_eq!(recipe["instructions"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn auth_unauthorized_access() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;

    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/recipes"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    let resp = client
        .post(format!("{base}/recipes"))
        .json(&json!({"title":"test","source":"test","ingredients":[],"instructions":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    let resp = client
        .get(format!("{base}/shopping"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    let resp = client
        .get(format!("{base}/meal-plan?day=2026-02-15"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn shopping_structured_update() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let created = client
        .post(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"text":"2 apples"}))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let id = created["id"].as_i64().expect("id");

    let updated = client
        .patch(format!("{base}/shopping/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"name":"bananas","quantity":5.0}))
        .send()
        .await
        .unwrap();
    assert_eq!(updated.status(), reqwest::StatusCode::OK);

    let item = updated.json::<serde_json::Value>().await.unwrap();
    assert!(item["text"].as_str().unwrap().contains("banana"));
}

#[tokio::test]
async fn recipes_patch_updates() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let created = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "title": "Original",
            "source": "test",
            "yield": "2 servings",
            "ingredients": [],
            "instructions": []
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let id = created["id"].as_i64().expect("id");

    let updated = client
        .patch(format!("{base}/recipes/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"yield":"4 servings"}))
        .send()
        .await
        .unwrap();
    assert_eq!(updated.status(), reqwest::StatusCode::OK);

    let recipe = updated.json::<serde_json::Value>().await.unwrap();
    assert_eq!(recipe["yield"], "4 servings");
    assert_eq!(recipe["title"], "Original");
}

// ───────────────────────────── version ─────────────────────────────

#[tokio::test]
async fn version_ok() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;

    let resp = reqwest::get(format!("{base}/version"))
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    assert!(
        resp.get("version").and_then(|v| v.as_str()).is_some(),
        "expected version field, got: {resp}"
    );
}

// ───────────────────────── auth edge cases ──────────────────────────

#[tokio::test]
async fn auth_invalid_bearer_token() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;

    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/recipes"))
        .header("Authorization", "Bearer this-is-not-a-valid-jwt")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_malformed_authorization_header() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;

    let client = reqwest::Client::new();

    // Missing "Bearer " prefix
    let resp = client
        .get(format!("{base}/recipes"))
        .header("Authorization", "notbearer token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}

// ──────────────────────── recipes extra tests ───────────────────────

#[tokio::test]
async fn recipes_list_initially_empty() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let list = client
        .get(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    assert_eq!(
        list.as_array().unwrap().len(),
        0,
        "fresh server should have no recipes"
    );
}

#[tokio::test]
async fn recipes_patch_ingredients_and_instructions() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let created = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "title": "To Patch",
            "source": "test",
            "ingredients": [{"name": "flour", "quantity": 100.0, "unit": "g"}],
            "instructions": ["Mix"]
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let id = created["id"].as_i64().expect("id");

    let updated = client
        .patch(format!("{base}/recipes/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "ingredients": [
                {"name": "flour", "quantity": 200.0, "unit": "g"},
                {"name": "eggs", "quantity": 2.0}
            ],
            "instructions": ["Mix", "Bake", "Cool"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(updated.status(), reqwest::StatusCode::OK);

    let recipe = updated.json::<serde_json::Value>().await.unwrap();
    assert_eq!(recipe["ingredients"].as_array().unwrap().len(), 2);
    assert_eq!(recipe["instructions"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn recipes_patch_with_empty_ingredient_name_rejected() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let created = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "title": "Test",
            "source": "test",
            "ingredients": [],
            "instructions": []
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let id = created["id"].as_i64().expect("id");

    let resp = client
        .patch(format!("{base}/recipes/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({ "ingredients": [{"name": "", "quantity": 1.0}] }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn recipes_create_with_ingredient_empty_name_rejected() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "title": "Test",
            "source": "test",
            "ingredients": [{"name": "  ", "quantity": 1.0}],
            "instructions": []
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn recipes_delete_then_get_returns_not_found() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let created = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "title": "To Delete",
            "source": "test",
            "ingredients": [],
            "instructions": []
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let id = created["id"].as_i64().expect("id");

    client
        .delete(format!("{base}/recipes/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    let resp = client
        .get(format!("{base}/recipes/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn recipes_list_ordering() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let mut ids = Vec::new();
    for title in &["Alpha", "Beta", "Gamma"] {
        let r = client
            .post(format!("{base}/recipes"))
            .header("Authorization", format!("Bearer {token}"))
            .json(&json!({"title": title, "source": "test", "ingredients": [], "instructions": []}))
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap();
        ids.push(r["id"].as_i64().expect("id"));
    }

    let list = client
        .get(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    let returned_ids: Vec<i64> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_i64().unwrap())
        .collect();

    // Should be ordered ascending by id
    let mut sorted = returned_ids.clone();
    sorted.sort();
    assert_eq!(returned_ids, sorted, "recipes should be ordered by id");
}

#[tokio::test]
async fn recipes_source_and_notes_persisted() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let created = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "title": "Sourced",
            "source": "https://example.com/recipe",
            "notes": "Grandma's secret",
            "ingredients": [],
            "instructions": []
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    assert_eq!(created["source"], "https://example.com/recipe");
    assert_eq!(created["notes"], "Grandma's secret");
}

#[tokio::test]
async fn recipes_patch_source_and_notes() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let created = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "title": "Patch notes test",
            "source": "old-source",
            "notes": "old notes",
            "ingredients": [],
            "instructions": []
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let id = created["id"].as_i64().expect("id");

    let updated = client
        .patch(format!("{base}/recipes/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"source": "new-source", "notes": "new notes"}))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    assert_eq!(updated["source"], "new-source");
    assert_eq!(updated["notes"], "new notes");
}

// ─────────────────────────── share recipe ───────────────────────────

#[tokio::test]
async fn share_recipe_create_and_fetch() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let recipe = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "title": "Sharable Recipe",
            "source": "test",
            "ingredients": [{"name": "flour", "quantity": 200.0, "unit": "g"}],
            "instructions": ["Mix and bake"]
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let id = recipe["id"].as_i64().expect("id");

    // Create share token
    let share = client
        .post(format!("{base}/recipes/{id}/share"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(share.status(), reqwest::StatusCode::OK);
    let share_js = share.json::<serde_json::Value>().await.unwrap();
    let share_token = share_js["share_token"].as_str().expect("share_token");
    assert!(!share_token.is_empty());

    // Fetch via public share URL (no auth)
    let public = reqwest::get(format!("{base}/api/share/{share_token}"))
        .await
        .unwrap();
    assert_eq!(public.status(), reqwest::StatusCode::OK);
    let public_recipe = public.json::<serde_json::Value>().await.unwrap();
    assert_eq!(public_recipe["title"], "Sharable Recipe");
    assert_eq!(public_recipe["ingredients"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn share_recipe_idempotent() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let recipe = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"title": "IdempotentShare", "source": "test", "ingredients": [], "instructions": []}))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let id = recipe["id"].as_i64().expect("id");

    let first = client
        .post(format!("{base}/recipes/{id}/share"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let second = client
        .post(format!("{base}/recipes/{id}/share"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    // Both calls should return the same token
    assert_eq!(first["share_token"], second["share_token"]);
}

#[tokio::test]
async fn share_recipe_revoke_and_404() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let recipe = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"title": "ToRevoke", "source": "test", "ingredients": [], "instructions": []}))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let id = recipe["id"].as_i64().expect("id");

    let share_js = client
        .post(format!("{base}/recipes/{id}/share"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let share_token = share_js["share_token"].as_str().unwrap().to_string();

    // Revoke
    let revoke = client
        .delete(format!("{base}/recipes/{id}/share"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(revoke.status(), reqwest::StatusCode::NO_CONTENT);

    // Fetching revoked token should 404
    let resp = reqwest::get(format!("{base}/api/share/{share_token}"))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn share_recipe_unknown_token_404() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;

    let resp = reqwest::get(format!("{base}/api/share/does-not-exist-at-all"))
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn share_recipe_nonexistent_recipe_404() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/recipes/99999/share"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

// ──────────────────────── shopping extra tests ──────────────────────

#[tokio::test]
async fn shopping_list_all_texts() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    // Create a couple of items
    for text in &["3 bananas", "500 g rice"] {
        client
            .post(format!("{base}/shopping"))
            .header("Authorization", format!("Bearer {token}"))
            .json(&json!({"text": text}))
            .send()
            .await
            .unwrap();
    }

    let all_texts = client
        .get(format!("{base}/shopping/all-texts"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    let arr = all_texts.as_array().expect("expected array");
    assert!(!arr.is_empty(), "all-texts should have entries");
}

#[tokio::test]
async fn shopping_delete_nonexistent_item() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{base}/shopping/99999"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    // Deleting a non-existent item should not 500
    assert!(
        resp.status() != reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        "deleting nonexistent shopping item should not 500"
    );
}

#[tokio::test]
async fn shopping_patch_nonexistent_item() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    // Patching a non-existent item: the server currently returns a 4xx/5xx error
    // (RowNotFound maps to INTERNAL_SERVER_ERROR). Ensure it at least errors.
    let resp = client
        .patch(format!("{base}/shopping/99999"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"done": true}))
        .send()
        .await
        .unwrap();
    assert!(
        !resp.status().is_success(),
        "patching a nonexistent shopping item should not succeed, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn shopping_merge_multiple_items() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let merged = client
        .post(format!("{base}/shopping/merge"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "items": [
                {"quantity": 2.0, "unit": null, "name": "avocados", "category": null},
                {"quantity": 300.0, "unit": "g", "name": "chicken", "category": null},
                {"quantity": null, "unit": null, "name": "garlic", "category": null}
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(merged.status(), reqwest::StatusCode::OK);

    let list = merged.json::<serde_json::Value>().await.unwrap();
    let arr = list.as_array().expect("expected array");
    assert!(arr.len() >= 3, "expected at least 3 items in list");
}

#[tokio::test]
async fn shopping_list_excludes_done_items() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let item = client
        .post(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"text": "4 oranges"}))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let id = item["id"].as_i64().expect("id");

    // Mark as done
    client
        .patch(format!("{base}/shopping/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"done": true}))
        .send()
        .await
        .unwrap();

    let list = client
        .get(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    assert!(
        list.as_array().unwrap().iter().all(|x| x["id"] != id),
        "done item must not appear in shopping list"
    );
}

#[tokio::test]
async fn shopping_undone_item_reappears() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let item = client
        .post(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"text": "2 lemons"}))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let id = item["id"].as_i64().expect("id");

    // Mark done then undone
    client
        .patch(format!("{base}/shopping/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"done": true}))
        .send()
        .await
        .unwrap();
    client
        .patch(format!("{base}/shopping/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"done": false}))
        .send()
        .await
        .unwrap();

    let list = client
        .get(format!("{base}/shopping"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    assert!(
        list.as_array().unwrap().iter().any(|x| x["id"] == id),
        "undone item should reappear in shopping list"
    );
}

// ─────────────────────────── meal plan extra ────────────────────────

#[tokio::test]
async fn meal_plan_empty_day_returns_empty_list() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let day_plan = client
        .get(format!("{base}/meal-plan?day=2099-12-31"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    assert_eq!(
        day_plan.as_array().unwrap().len(),
        0,
        "far-future day should have no meal plan entries"
    );
}

#[tokio::test]
async fn meal_plan_multiple_recipes_same_day() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let mut recipe_ids = Vec::new();
    for title in &["Breakfast", "Lunch", "Dinner"] {
        let r = client
            .post(format!("{base}/recipes"))
            .header("Authorization", format!("Bearer {token}"))
            .json(&json!({"title": title, "source": "test", "ingredients": [], "instructions": []}))
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap();
        recipe_ids.push(r["id"].as_i64().expect("id"));
    }

    for rid in &recipe_ids {
        let resp = client
            .post(format!("{base}/meal-plan"))
            .header("Authorization", format!("Bearer {token}"))
            .json(&json!({"day": "2026-03-01", "recipe_id": rid}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
    }

    let day_plan = client
        .get(format!("{base}/meal-plan?day=2026-03-01"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    assert_eq!(
        day_plan.as_array().unwrap().len(),
        3,
        "all three recipes should appear on 2026-03-01"
    );
}

#[tokio::test]
async fn meal_plan_delete_nonexistent_returns_zero_deleted() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{base}/meal-plan/2026-02-15/99999"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body = resp.json::<serde_json::Value>().await.unwrap();
    assert_eq!(body["deleted"].as_i64(), Some(0));
}

#[tokio::test]
async fn meal_plan_response_includes_recipe_title() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    let recipe = client
        .post(format!("{base}/recipes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"title": "Title Check", "source": "test", "ingredients": [], "instructions": []}))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    let recipe_id = recipe["id"].as_i64().expect("id");

    let assigned = client
        .post(format!("{base}/meal-plan"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"day": "2026-04-01", "recipe_id": recipe_id}))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    assert_eq!(assigned["title"], "Title Check");
    assert_eq!(assigned["recipe_id"], recipe_id);
    assert_eq!(assigned["day"], "2026-04-01");
}

#[tokio::test]
async fn ntfy_notified_on_client_error() {
    let mock = MockNtfy::start().await;
    let srv = TestServer::start_with_ntfy(&mock.url());
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = login(&base).await;

    let client = reqwest::Client::new();

    // PATCH a shopping item with an invalid category value.
    // Category validation runs before any DB access, so no real item is needed.
    // This returns AppError::Msg(400, "invalid category") → should trigger ntfy.
    let resp = client
        .patch(format!("{base}/shopping/1"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({"category": "not_a_real_category"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    let msgs = mock.wait_for_messages(1, 2000).await;
    assert!(
        !msgs.is_empty(),
        "expected ntfy notification for 400 error, got none"
    );
    assert!(
        msgs[0].contains("400"),
        "expected message to mention 400, got: {}",
        msgs[0]
    );
}

#[tokio::test]
async fn ntfy_not_notified_on_unauthorized() {
    let mock = MockNtfy::start().await;
    let srv = TestServer::start_with_ntfy(&mock.url());
    let base = srv.base_url();
    wait_ready(&base).await;

    let client = reqwest::Client::new();

    // Hit a protected route without a token → 401
    let resp = client
        .get(format!("{base}/recipes"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    // Give a generous window for any spurious notification to arrive
    tokio::time::sleep(Duration::from_millis(500)).await;

    let msgs = mock.received.lock().await.clone();
    assert!(
        msgs.is_empty(),
        "expected no ntfy notification for 401, got: {msgs:?}"
    );
}
