//! SQLite pool setup and migrations (ADR 0004).

use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::migrate::{MigrateError, Migrator};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

static MIGRATOR: Migrator = sqlx::migrate!();

/// Opens the database, creating the file if it is missing.
///
/// # Errors
///
/// Fails if the database cannot be opened.
pub async fn connect(options: SqliteConnectOptions) -> Result<SqlitePool, sqlx::Error> {
    let options = options
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    SqlitePoolOptions::new().connect_with(options).await
}

/// Applies any migrations the database has not seen yet.
///
/// # Errors
///
/// Fails if a migration fails or the applied history no longer matches the embedded one.
pub async fn migrate(pool: &SqlitePool) -> Result<(), MigrateError> {
    MIGRATOR.run(pool).await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn applied_migrations(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    fn options(path: &std::path::Path) -> SqliteConnectOptions {
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
    }

    #[tokio::test]
    async fn creates_and_migrates_a_fresh_database_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("asgard.db");
        let expected = i64::try_from(MIGRATOR.iter().count()).unwrap();

        let pool = connect(options(&path)).await.unwrap();
        migrate(&pool).await.unwrap();
        assert!(path.exists());
        assert_eq!(applied_migrations(&pool).await, expected);
        pool.close().await;

        let pool = connect(options(&path)).await.unwrap();
        migrate(&pool).await.unwrap();
        assert_eq!(applied_migrations(&pool).await, expected);
        pool.close().await;
    }
}
