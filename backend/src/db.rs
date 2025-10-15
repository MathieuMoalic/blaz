use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use std::path::PathBuf;

pub async fn make_pool() -> anyhow::Result<SqlitePool> {
    // default to ./blaz.sqlite, but allow override with DATABASE_PATH
    let db_path = std::env::var("BLAZ_DATABASE_PATH").unwrap_or_else(|_| "blaz.sqlite".into());
    let db_path = PathBuf::from(db_path);

    // ensure parent directory exists; WAL uses -wal/-shm files
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
