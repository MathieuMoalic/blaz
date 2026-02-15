use std::{
    net::TcpListener as StdTcpListener,
    process::{Child, Command, Stdio},
    time::Duration,
};

use serde_json::json;
use tempfile::TempDir;

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

impl TestServer {
    fn start() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let port = pick_free_port();

        let db_path = tmp.path().join("test.sqlite");
        let media_dir = tmp.path().join("media");
        let log_file = tmp.path().join("test.log");

        // Ensure dirs exist for cleaner startup
        std::fs::create_dir_all(&media_dir).ok();

        // NOTE: env!("CARGO_BIN_EXE_blaz") is provided by Cargo for integration tests
        // and points at the compiled binary.
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_blaz"));
        cmd.env("BLAZ_BIND_ADDR", format!("127.0.0.1:{port}"))
            .env("BLAZ_DATABASE_PATH", db_path.to_string_lossy().to_string())
            .env("BLAZ_MEDIA_DIR", media_dir.to_string_lossy().to_string())
            .env("BLAZ_LOG_FILE", log_file.to_string_lossy().to_string())
            // keep output quiet unless test fails
            .stdout(Stdio::null())
            .stderr(Stdio::null());

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

async fn register_and_login(base: &str) -> String {
    let client = reqwest::Client::new();

    client
        .post(format!("{base}/auth/register"))
        .json(&json!({"email":"test@example.com","password":"testpassword123"}))
        .send()
        .await
        .unwrap();

    let login = client
        .post(format!("{base}/auth/login"))
        .json(&json!({"email":"test@example.com","password":"testpassword123"}))
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
async fn auth_register_login_single_user_guard() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;

    let client = reqwest::Client::new();

    // Fresh DB => allow_registration = true
    let st = client
        .get(format!("{base}/auth/status"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();

    assert_eq!(st["allow_registration"], true);

    // Bad password => 400
    let resp = client
        .post(format!("{base}/auth/register"))
        .json(&json!({"email":"a@b.com","password":"short"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    // Good registration => 201
    let resp = client
        .post(format!("{base}/auth/register"))
        .json(&json!({"email":"user@example.com","password":"correcthorsebatterystaple"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);

    // Second registration attempt => 403 (single-user guard)
    let resp = client
        .post(format!("{base}/auth/register"))
        .json(&json!({"email":"other@example.com","password":"anothergoodpassword"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

    // Status now false
    let st = client
        .get(format!("{base}/auth/status"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(st["allow_registration"], false);

    // Login ok => returns token
    let login = client
        .post(format!("{base}/auth/login"))
        .json(&json!({"email":"user@example.com","password":"correcthorsebatterystaple"}))
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
async fn recipes_crud_smoke() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = register_and_login(&base).await;

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
    let token = register_and_login(&base).await;

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
    let token = register_and_login(&base).await;

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
async fn auth_bad_email_validation() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;

    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/auth/register"))
        .json(&json!({"email":"notanemail","password":"validpassword123"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    let resp = client
        .post(format!("{base}/auth/register"))
        .json(&json!({"email":"","password":"validpassword123"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    let resp = client
        .post(format!("{base}/auth/register"))
        .json(&json!({"email":"  ","password":"validpassword123"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn auth_password_length() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;

    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/auth/register"))
        .json(&json!({"email":"test@example.com","password":"1234567"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    let resp = client
        .post(format!("{base}/auth/register"))
        .json(&json!({"email":"test@example.com","password":"12345678"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
}

#[tokio::test]
async fn auth_login_failures() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;

    let client = reqwest::Client::new();

    client
        .post(format!("{base}/auth/register"))
        .json(&json!({"email":"test@example.com","password":"correctpassword"}))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{base}/auth/login"))
        .json(&json!({"email":"test@example.com","password":"wrongpassword"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    let resp = client
        .post(format!("{base}/auth/login"))
        .json(&json!({"email":"nonexistent@example.com","password":"anypassword"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn recipes_not_found() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;
    let token = register_and_login(&base).await;

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
    let token = register_and_login(&base).await;

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
    let token = register_and_login(&base).await;

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
    let token = register_and_login(&base).await;

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
    let token = register_and_login(&base).await;

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
    let token = register_and_login(&base).await;

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
    let token = register_and_login(&base).await;

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
    let token = register_and_login(&base).await;

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
    let token = register_and_login(&base).await;

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
    let token = register_and_login(&base).await;

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
    let token = register_and_login(&base).await;

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
    let token = register_and_login(&base).await;

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
    let token = register_and_login(&base).await;

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
