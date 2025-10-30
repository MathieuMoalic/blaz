use std::{net::SocketAddr, path::Path};
use tokio::net::TcpListener;

use blaz::{build_app, db::make_pool, init_logging, models::AppState};
use sqlx::migrate::Migrator;

fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(v) => {
            let s = v.trim().to_ascii_lowercase();
            matches!(s.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => default,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Unified logging setup with sane defaults (can still override via RUST_LOG)
    init_logging();

    let pool = make_pool().await?;
    Migrator::new(Path::new("./migrations"))
        .await?
        .run(&pool)
        .await?;

    let media_dir = std::env::var("BLAZ_MEDIA_DIR").unwrap_or_else(|_| "media".into());
    let media_dir = std::path::PathBuf::from(media_dir);
    tokio::fs::create_dir_all(&media_dir).await.ok();

    // Feature flag: default ON for dev convenience
    // Set BLAZ_ALLOW_REGISTRATION=false to disable new signups.
    let allow_registration = env_bool("BLAZ_ALLOW_REGISTRATION", true);

    let secret = std::env::var("BLAZ_JWT_SECRET").unwrap_or_else(|_| "dev-secret-change-me".into());

    let state = AppState {
        pool,
        media_dir,
        jwt_encoding: jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
        jwt_decoding: jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        allow_registration,
    };

    let app = build_app(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
