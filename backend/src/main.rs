use axum::{
    Router,
    extract::{Json, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::{FromRow, SqlitePool};
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::net::TcpListener;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
struct AppState {
    pool: SqlitePool,
}

#[derive(Serialize, Deserialize, FromRow, Clone)]
struct Recipe {
    id: i64,
    title: String,
}

#[derive(Deserialize)]
struct NewRecipe {
    title: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // logging
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "blaz=info,axum=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let pool = make_pool().await?;
    sqlx::migrate!("./migrations").run(&pool).await?;

    let state = AppState { pool };

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/recipes", get(list_recipes).post(create_recipe))
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    tracing::info!("listening on http://{addr}");

    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn list_recipes(
    State(state): State<AppState>,
) -> Result<Json<Vec<Recipe>>, axum::http::StatusCode> {
    let rows: Vec<Recipe> =
        sqlx::query_as::<_, Recipe>("SELECT id, title FROM recipes ORDER BY id")
            .fetch_all(&state.pool)
            .await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(rows))
}

async fn create_recipe(
    State(state): State<AppState>,
    Json(new): Json<NewRecipe>,
) -> Result<Json<Recipe>, axum::http::StatusCode> {
    let rec: Recipe =
        sqlx::query_as::<_, Recipe>("INSERT INTO recipes(title) VALUES (?) RETURNING id, title")
            .bind(new.title)
            .fetch_one(&state.pool)
            .await
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(rec))
}

async fn make_pool() -> anyhow::Result<SqlitePool> {
    // default to ./blaz.sqlite, but allow override with DATABASE_PATH
    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "blaz.sqlite".into());
    let db_path = PathBuf::from(db_path);

    // ensure parent directory exists (important: WAL needs to create -wal/-shm files)
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let opts = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);

    Ok(SqlitePool::connect_with(opts).await?)
}
