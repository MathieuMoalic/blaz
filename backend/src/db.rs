use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use std::path::PathBuf;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// # Errors
///
/// Will return `Err` if the `database_path` is not writable, or a connection can't be made to the db
/// file
pub async fn make_pool(database_path: String) -> anyhow::Result<SqlitePool> {
    let db_path = PathBuf::from(database_path);

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
