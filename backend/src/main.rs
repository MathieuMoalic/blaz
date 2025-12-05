use std::sync::Arc;

use clap::Parser;
use tokio::net::TcpListener;
use tokio::sync::RwLock;

use blaz::{
    build_app,
    config::Config,
    db::make_pool,
    init_logging,
    models::{AppSettings, AppState},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::parse();
    init_logging();

    let pool = make_pool(config.database_path.clone()).await?;

    tokio::fs::create_dir_all(&config.media_dir).await.ok();

    let settings = load_settings(&pool).await?;
    let jwt_secret = settings.jwt_secret.clone();
    let settings = Arc::new(RwLock::new(settings.clone()));

    let state = AppState {
        pool,
        jwt_encoding: jsonwebtoken::EncodingKey::from_secret(jwt_secret.as_bytes()),
        jwt_decoding: jsonwebtoken::DecodingKey::from_secret(jwt_secret.as_bytes()),
        settings,
        config: config.clone(),
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
