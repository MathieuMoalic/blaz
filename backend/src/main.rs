use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tokio::sync::RwLock;

use blaz::{
    build_app,
    db::make_pool,
    init_logging,
    models::{AppSettings, AppState},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // logging
    init_logging();

    // DB pool (already runs *embedded* migrations inside make_pool)
    let pool = make_pool().await?;

    // Media dir
    let media_dir = std::env::var("BLAZ_MEDIA_DIR").unwrap_or_else(|_| "media".into());
    let media_dir = std::path::PathBuf::from(media_dir);
    tokio::fs::create_dir_all(&media_dir).await.ok();

    let settings = load_settings(&pool).await?;
    let jwt_secret = settings.jwt_secret.clone();
    let settings = Arc::new(RwLock::new(settings.clone()));

    let state = AppState {
        pool,
        media_dir,
        jwt_encoding: jsonwebtoken::EncodingKey::from_secret(jwt_secret.as_bytes()),
        jwt_decoding: jsonwebtoken::DecodingKey::from_secret(jwt_secret.as_bytes()),
        settings,
    };

    let app = build_app(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = TcpListener::bind(addr).await?;
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
