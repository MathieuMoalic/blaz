use std::{net::SocketAddr, path::Path, sync::Arc};
use tokio::net::TcpListener;
use tokio::sync::RwLock;

use blaz::{
    build_app,
    db::make_pool,
    init_logging,
    models::{AppSettings, AppState, SettingsRow},
};
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

    // DB pool + migrations
    let pool = make_pool().await?;
    Migrator::new(Path::new("./migrations"))
        .await?
        .run(&pool)
        .await?;

    // Media dir
    let media_dir = std::env::var("BLAZ_MEDIA_DIR").unwrap_or_else(|_| "media".into());
    let media_dir = std::path::PathBuf::from(media_dir);
    tokio::fs::create_dir_all(&media_dir).await.ok();

    // Load or initialize editable settings (singleton id=1)
    let settings = load_or_init_settings(&pool).await?;
    let settings = Arc::new(RwLock::new(settings));

    // JWT
    let secret = std::env::var("BLAZ_JWT_SECRET").unwrap_or_else(|_| "dev-secret-change-me".into());

    // App state
    let state = AppState {
        pool,
        media_dir,
        jwt_encoding: jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
        jwt_decoding: jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        settings,
    };

    // Router
    let app = build_app(state);

    // Serve
    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn load_or_init_settings(pool: &sqlx::SqlitePool) -> anyhow::Result<AppSettings> {
    // Try load existing singleton row
    if let Some(row) = sqlx::query_as::<_, SettingsRow>(
        r#"
        SELECT llm_api_key, llm_model, llm_api_url, allow_registration,
               system_prompt_import, system_prompt_macros
          FROM settings
         WHERE id = 1
        "#,
    )
    .fetch_optional(pool)
    .await?
    {
        return Ok(row.into());
    }

    // Bootstrap from environment on first run
    let init = AppSettings {
        llm_api_key: std::env::var("BLAZ_LLM_API_KEY").ok(),
        llm_model: std::env::var("BLAZ_LLM_MODEL")
            .unwrap_or_else(|_| "meta-llama/Llama-3.1-8B-Instruct".into()),
        llm_api_url: std::env::var("BLAZ_LLM_API_URL")
            .unwrap_or_else(|_| "https://router.huggingface.co/v1".into()),
        allow_registration: env_bool("BLAZ_ALLOW_REGISTRATION", true),
        system_prompt_import: String::new(), // default to built-in prompt
        system_prompt_macros: String::new(), // default to built-in prompt
    };

    // Persist to DB (singleton id=1)
    sqlx::query(
        r#"
        INSERT INTO settings (
            id, llm_api_key, llm_model, llm_api_url, allow_registration,
            system_prompt_import, system_prompt_macros
        )
        VALUES (1, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&init.llm_api_key)
    .bind(&init.llm_model)
    .bind(&init.llm_api_url)
    .bind(if init.allow_registration {
        1_i64
    } else {
        0_i64
    })
    .bind(&init.system_prompt_import)
    .bind(&init.system_prompt_macros)
    .execute(pool)
    .await?;

    Ok(init)
}
