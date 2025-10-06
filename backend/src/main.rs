use std::{net::SocketAddr, path::Path};
use tokio::net::TcpListener;

use blaz::{build_app, db::make_pool, models::AppState};
use sqlx::migrate::Migrator;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // logging
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "blaz=info,axum=info,sqlx=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // DB + migrations (no macros)
    let pool = make_pool().await?;
    Migrator::new(Path::new("./migrations"))
        .await?
        .run(&pool)
        .await?;

    let media_dir = std::env::var("MEDIA_DIR").unwrap_or_else(|_| "media".into());
    let media_dir = std::path::PathBuf::from(media_dir);
    tokio::fs::create_dir_all(&media_dir).await.ok(); // best-effort
    let state = AppState { pool, media_dir };
    let app = build_app(state);

    // serve
    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
