use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::json;
use tower::ServiceExt;

use blaz::{build_app, models::AppState};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::{SqlitePool, migrate::Migrator};
use std::path::Path;

struct TestCtx {
    _tmp: tempfile::TempDir,
    app: Router,
}

async fn make_ctx() -> anyhow::Result<TestCtx> {
    let tmp = tempfile::tempdir()?;
    let db_path = tmp.path().join("test.sqlite");

    let opts = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        // avoid WAL in tests; simple file db is fine
        .journal_mode(SqliteJournalMode::Delete)
        .synchronous(SqliteSynchronous::Off);

    let pool = SqlitePool::connect_with(opts).await?;
    // run migrations from repo
    let migrator = Migrator::new(Path::new("./migrations")).await?;
    migrator.run(&pool).await?;

    let state = AppState { pool: pool.clone() };
    let app = build_app(state);

    Ok(TestCtx { _tmp: tmp, app })
}

async fn json_req(app: &Router, req: Request<Body>) -> (StatusCode, serde_json::Value) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"_raw": String::from_utf8_lossy(&bytes)}))
    };
    (status, body)
}

#[tokio::test]
async fn healthz_ok() -> anyhow::Result<()> {
    let ctx = make_ctx().await?;
    let (st, body) = json_req(
        &ctx.app,
        Request::get("/healthz").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body, json!("ok"));
    Ok(())
}

#[tokio::test]
async fn recipes_crud() -> anyhow::Result<()> {
    let ctx = make_ctx().await?;

    // create
    let (st, body) = json_req(
        &ctx.app,
        Request::post("/recipes")
            .header("content-type", "application/json")
            .body(Body::from(json!({"title":"Kouign-amann"}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let id = body.get("id").and_then(|v| v.as_i64()).unwrap();

    // get one
    let (st, body) = json_req(
        &ctx.app,
        Request::get(format!("/recipes/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["title"], "Kouign-amann");

    // list
    let (st, body) = json_req(
        &ctx.app,
        Request::get("/recipes").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(body.as_array().unwrap().iter().any(|r| r["id"] == id));

    // delete
    let (st, _body) = json_req(
        &ctx.app,
        Request::delete(format!("/recipes/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    Ok(())
}

#[tokio::test]
async fn meal_plan_assign_get_unassign() -> anyhow::Result<()> {
    let ctx = make_ctx().await?;
    // create recipe
    let (st, body) = json_req(
        &ctx.app,
        Request::post("/recipes")
            .header("content-type", "application/json")
            .body(Body::from(json!({"title":"Kig ha farz"}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let rid = body["id"].as_i64().unwrap();

    // assign
    let day = "2025-10-05";
    let (st, body) = json_req(
        &ctx.app,
        Request::post("/meal-plan")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"day": day, "recipe_id": rid}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["day"], day);

    // get
    let (st, body) = json_req(
        &ctx.app,
        Request::get(format!("/meal-plan?day={day}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["recipe_id"], rid);

    // unassign
    let (st, _body) = json_req(
        &ctx.app,
        Request::delete(format!("/meal-plan/{day}/{rid}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    Ok(())
}

#[tokio::test]
async fn shopping_crud_toggle() -> anyhow::Result<()> {
    let ctx = make_ctx().await?;

    // create
    let (st, body) = json_req(
        &ctx.app,
        Request::post("/shopping")
            .header("content-type", "application/json")
            .body(Body::from(json!({"text":"1 kg flour"}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let id = body["id"].as_i64().unwrap();
    assert_eq!(body["done"].as_i64().unwrap(), 0);

    // list
    let (st, body) = json_req(
        &ctx.app,
        Request::get("/shopping").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(body.as_array().unwrap().iter().any(|x| x["id"] == id));

    // toggle done
    let (st, body) = json_req(
        &ctx.app,
        Request::patch(format!("/shopping/{id}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({"done": true}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["done"].as_i64().unwrap(), 1);

    // delete
    let (st, _body) = json_req(
        &ctx.app,
        Request::delete(format!("/shopping/{id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    Ok(())
}
