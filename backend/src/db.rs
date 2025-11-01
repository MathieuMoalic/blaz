use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::{Pool, Sqlite, SqlitePool};
use std::path::PathBuf;

// Embedded migrations (compiled in)
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

// Optional: keep if you use a DATABASE_URL path somewhere else
pub async fn init_pool(database_url: &str) -> Result<Pool<Sqlite>, sqlx::Error> {
    let pool = SqlitePool::connect(database_url).await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

pub async fn make_pool() -> anyhow::Result<SqlitePool> {
    // default to ./blaz.sqlite, override with BLAZ_DATABASE_PATH
    let db_path = std::env::var("BLAZ_DATABASE_PATH").unwrap_or_else(|_| "blaz.sqlite".into());
    let db_path = PathBuf::from(db_path);

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let opts = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);

    // Connect, then **run migrations**
    let pool = SqlitePool::connect_with(opts).await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}
