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

    let client = reqwest::Client::new();

    // Create
    let created = client
        .post(format!("{base}/recipes"))
        .json(&json!({
            "title": "Test Pasta",
            "source": "unit-test",
            "yield": "2 servings",
            "notes": "",
            "ingredients": ["200 g pasta", "1 tbsp olive oil", "salt"],
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
        .send()
        .await
        .unwrap();
    assert_eq!(got.status(), reqwest::StatusCode::OK);

    // List contains it
    let list = client
        .get(format!("{base}/recipes"))
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
        .json(&json!({"title":"Test Pasta Updated"}))
        .send()
        .await
        .unwrap();
    assert_eq!(upd.status(), reqwest::StatusCode::OK);

    // Delete
    let del = client
        .delete(format!("{base}/recipes/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), reqwest::StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn shopping_create_and_merge_smoke() {
    let srv = TestServer::start();
    let base = srv.base_url();
    wait_ready(&base).await;

    let client = reqwest::Client::new();

    // Create simple quantity item
    let item = client
        .post(format!("{base}/shopping"))
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
