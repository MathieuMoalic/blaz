#![deny(
    warnings,
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::cargo
)]
#![allow(clippy::multiple_crate_versions)]

mod app;
mod auth_middleware;
mod categories;
mod config;
mod db;
mod error;
mod html;
mod image_io;
mod llm;
mod logging;
mod models;
mod routes;
mod units;

use clap::Parser;
use tokio::net::TcpListener;

use crate::{
    app::build_app,
    config::{Cli, Commands},
    db::make_pool,
    logging::init_logging,
    models::AppState,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Handle subcommands
    if let Some(command) = cli.command {
        return handle_command(command);
    }

    let mut config = cli.config;

    // Keep guard alive so file logger flushes correctly
    let _log_guards = init_logging(&config);

    // Generate JWT secret if not provided
    if config.jwt_secret.is_none() {
        use rand::Rng;
        let secret: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();
        tracing::warn!(
            "No JWT secret provided, generated random secret (tokens will be invalid on restart)"
        );
        config.jwt_secret = Some(secret);
    }

    // Log all configuration (mask sensitive values)
    tracing::info!("=== Configuration ===");
    tracing::info!("Bind address: {}", config.bind);
    tracing::info!("Media directory: {}", config.media_dir.display());
    tracing::info!("Database path: {}", config.database_path);
    tracing::info!("Log file: {}", config.log_file.display());
    tracing::info!(
        "CORS origin: {}",
        config.cors_origin.as_deref().unwrap_or("<allow all>")
    );
    tracing::info!(
        "JWT secret: {}",
        if config.jwt_secret.is_some() {
            "<set>"
        } else {
            "<not set>"
        }
    );
    tracing::info!(
        "LLM API key: {}",
        if config.llm_api_key.as_ref().is_some_and(|k| !k.is_empty()) {
            "<set>"
        } else {
            "<not set>"
        }
    );
    tracing::info!("LLM model: {}", config.llm_model);
    tracing::info!("LLM API URL: {}", config.llm_api_url);
    tracing::info!(
        "System prompt import: {} chars",
        config.system_prompt_import.len()
    );
    tracing::info!(
        "System prompt macros: {} chars",
        config.system_prompt_macros.len()
    );
    tracing::info!(
        "Password hash: {}",
        if config.password_hash.is_some() {
            "<set>"
        } else {
            "<not set>"
        }
    );
    tracing::info!("====================");

    let pool = make_pool(config.database_path.clone()).await?;
    tokio::fs::create_dir_all(&config.media_dir).await.ok();

    let jwt_secret = config.jwt_secret.as_ref().unwrap();
    let state = AppState {
        pool,
        jwt_encoding: jsonwebtoken::EncodingKey::from_secret(jwt_secret.as_bytes()),
        config: config.clone(),
    };

    let app = build_app(state);

    let listener = TcpListener::bind(config.bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn handle_command(command: Commands) -> anyhow::Result<()> {
    match command {
        Commands::HashPassword => hash_password_interactive(),
    }
}

fn hash_password_interactive() -> anyhow::Result<()> {
    use argon2::Argon2;
    use password_hash::{PasswordHasher, SaltString};
    use rand::rngs::OsRng;

    println!("Enter password to hash:");
    let password = rpassword::read_password()?;

    if password.trim().is_empty() {
        anyhow::bail!("Password cannot be empty");
    }

    if password.len() < 8 {
        anyhow::bail!("Password must be at least 8 characters");
    }

    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("Failed to hash password: {e}"))?
        .to_string();

    println!("\nArgon2 password hash:");
    println!("{hash}");
    println!("\nAdd this to your .env file:");
    println!("BLAZ_PASSWORD_HASH=\"{hash}\"");

    Ok(())
}
