use std::path::{Path, PathBuf};

use anyhow::Context;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

mod migrations;

pub const DATABASE_CONNECTIONS_MAX: u32 = 4;
pub const DATABASE_WAIT: std::time::Duration = std::time::Duration::from_secs(5);
pub const MIGRATION_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

pub fn database_path() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("PWF_DATABASE_PATH").filter(|path| !path.is_empty()) {
        return Ok(path.into());
    }

    let base_directories = directories::BaseDirs::new()
        .context("resolving the platform local data directory for the pwf database")?;
    Ok(database_path_in(base_directories.data_local_dir()))
}

pub fn database_path_in(root: &Path) -> PathBuf {
    root.join("pwf").join("pwf.sqlite3")
}

pub async fn build_pool(path: &Path) -> anyhow::Result<SqlitePool> {
    let parent = path
        .parent()
        .context("database path must have a parent directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating database directory {}", parent.display()))?;

    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(DATABASE_WAIT);

    SqlitePoolOptions::new()
        .max_connections(DATABASE_CONNECTIONS_MAX)
        .acquire_timeout(DATABASE_WAIT)
        .connect_with(options)
        .await
        .with_context(|| format!("connecting to database {}", path.display()))
}

pub async fn migrate_database(pool: &SqlitePool) -> anyhow::Result<()> {
    tokio::time::timeout(MIGRATION_WAIT, migrations::MIGRATOR.run(pool))
        .await
        .context("database migrations timed out")?
        .context("applying database migrations")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::build_pool;

    #[tokio::test]
    async fn pool_enables_foreign_keys_and_wal_journal_mode() {
        let directory = tempfile::tempdir().expect("create database directory");
        let pool = build_pool(&directory.path().join("pwf.sqlite3"))
            .await
            .expect("build SQLite pool");

        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .expect("read foreign-key mode");
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .expect("read journal mode");

        assert_eq!(foreign_keys, 1);
        assert_eq!(journal_mode, "wal");
    }
}
