use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use clap::Parser;
use tokio::net::TcpListener;
use tokio::sync::RwLock;

use blaz::{
    build_app,
    config::{self, Config},
    db::make_pool,
    init_logging,
    models::{AppSettings, AppState},
};

/// Blaz server
#[derive(Parser, Debug)]
#[command(
    name = "blaz-server",
    version,
    about = "HTTP API server for Blaz",
    long_about = None
)]
struct Cli {
    /// Address to bind the HTTP server to
    ///
    /// Example: 0.0.0.0:8080 or 127.0.0.1:3000
    #[arg(long, env = "BLAZ_BIND_ADDR", default_value = "0.0.0.0:8080")]
    bind: SocketAddr,

    /// Directory to store media files
    #[arg(long, env = "BLAZ_MEDIA_DIR", default_value = "media")]
    media_dir: PathBuf,

    /// Database URL (forwarded to make_pool via DATABASE_URL env)
    ///
    /// If not provided, whatever env var make_pool already uses
    /// (e.g. DATABASE_URL) continues to work as before.
    #[arg(long, env = "DATABASE_URL")]
    database_url: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI flags + env vars
    let config = Config::parse();

    // logging
    init_logging();

    // DB pool (already runs *embedded* migrations inside make_pool)
    let pool = make_pool().await?;

    // Media dir (from CLI/env, no manual env::var needed anymore)
    tokio::fs::create_dir_all(&config.media_dir).await.ok();

    let settings = load_settings(&pool).await?;
    let jwt_secret = settings.jwt_secret.clone();
    let settings = Arc::new(RwLock::new(settings.clone()));

    let state = AppState {
        pool,
        media_dir: config.media_dir.clone(),
        jwt_encoding: jsonwebtoken::EncodingKey::from_secret(jwt_secret.as_bytes()),
        jwt_decoding: jsonwebtoken::DecodingKey::from_secret(jwt_secret.as_bytes()),
        settings,
        config,
    };

    let app = build_app(state);

    // Bind address from CLI/env
    let listener = TcpListener::bind(config.bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn load_settings(pool: &sqlx::SqlitePool) -> anyhow::Result<AppSettings> {
    let settings = sqlx::query_as::<_, AppSettings>(
        r#"
        SELECT llm_api_key,
               llm_model,
               llm_api_url,
               jwt_secret,
               system_prompt_import,
               system_prompt_macros
          FROM settings
         WHERE id = 1
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(settings)
}
