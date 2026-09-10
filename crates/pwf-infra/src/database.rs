use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Context;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

mod migrations;

pub use migrations::{
    MigrationCompatibility, check_database_compatible, check_database_ready, migrate_database,
};

const DATABASE_CONNECTIONS_MAX: u32 = 4;
const DATABASE_MIGRATION_CONNECTIONS_MAX: u32 = 1;
const DATABASE_WAIT_MAX: Duration = Duration::from_secs(5);

pub fn database_path() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("PWF_DATABASE_PATH").filter(|path| !path.is_empty()) {
        return Ok(path.into());
    }

    let base_directories = directories::BaseDirs::new()
        .context("resolving the platform local data directory for the pwf database")?;
    Ok(database_path_in(base_directories.data_local_dir()))
}

#[must_use]
pub fn database_path_in(root: &Path) -> PathBuf {
    root.join("pwf").join("pwf.sqlite3")
}

pub async fn build_pool(path: &Path) -> anyhow::Result<SqlitePool> {
    let options = connect_options(path).create_if_missing(true);
    connect_pool(path, options, DATABASE_CONNECTIONS_MAX).await
}

pub async fn build_migration_pool(path: &Path) -> anyhow::Result<SqlitePool> {
    let options = connect_options(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal);
    connect_pool(path, options, DATABASE_MIGRATION_CONNECTIONS_MAX).await
}

/// Opens an existing database without creating files or permitting writes.
pub async fn build_read_only_pool(path: &Path) -> anyhow::Result<SqlitePool> {
    anyhow::ensure!(
        !path.is_dir(),
        "unable to open database file: {} is a directory",
        path.display()
    );
    let options = connect_options(path).read_only(true);
    SqlitePoolOptions::new()
        .max_connections(DATABASE_MIGRATION_CONNECTIONS_MAX)
        .acquire_timeout(DATABASE_WAIT_MAX)
        .connect_with(options)
        .await
        .with_context(|| format!("opening database read-only at {}", path.display()))
}

fn connect_options(path: &Path) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .foreign_keys(true)
        .busy_timeout(DATABASE_WAIT_MAX)
}

async fn connect_pool(
    path: &Path,
    options: SqliteConnectOptions,
    connections_max: u32,
) -> anyhow::Result<SqlitePool> {
    let parent = path
        .parent()
        .context("database path must have a parent directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating database directory {}", parent.display()))?;

    tokio::time::timeout(DATABASE_WAIT_MAX, async {
        loop {
            let result = SqlitePoolOptions::new()
                .max_connections(connections_max)
                .acquire_timeout(DATABASE_WAIT_MAX)
                .connect_with(options.clone())
                .await;
            match result {
                Err(sqlx::Error::Database(error))
                    if matches!(error.code().as_deref(), Some("5" | "6")) =>
                {
                    // Concurrent first opens can contend while SQLite enables WAL.
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                result => return result,
            }
        }
    })
    .await
    .with_context(|| format!("database connection window elapsed at {}", path.display()))?
    .with_context(|| format!("connecting to database {}", path.display()))
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;

    use super::{
        build_migration_pool, build_pool, build_read_only_pool, check_database_ready,
        migrate_database,
    };

    #[tokio::test]
    async fn read_only_pool_does_not_create_a_missing_database_or_parent() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path().join("missing");
        let path = parent.join("pwf.sqlite3");

        assert!(build_read_only_pool(&path).await.is_err());
        assert!(!parent.exists());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn pools_enable_foreign_keys_and_persist_wal_journal_mode() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("pwf.sqlite3");
        let migration_pool = build_migration_pool(&path).await.unwrap();
        migration_pool.close().await;
        let pool = build_pool(&path).await.unwrap();

        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(foreign_keys, 1);
        assert_eq!(journal_mode, "wal");
    }

    #[tokio::test]
    async fn initial_schema_enforces_current_constraints() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        migrate_database(&pool).await.unwrap();
        sqlx::query("INSERT INTO project_sources (kind, value) VALUES ('directory', '/work/foo')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO projects (id, project_source_id, title, tasks_kind, tasks_path) VALUES ('FOO', 1, 'foo', 'directory', '/tasks/foo')")
            .execute(&pool).await.unwrap();
        assert!(
            sqlx::query("UPDATE projects SET id = 'lower'")
                .execute(&pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query("UPDATE projects SET project_source_id = 99")
                .execute(&pool)
                .await
                .is_err()
        );
        sqlx::query("UPDATE projects SET project_source_id = NULL, obsidian_vault = '/notes'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            sqlx::query("UPDATE projects SET obsidian_vault = ' '")
                .execute(&pool)
                .await
                .is_err()
        );
        sqlx::query("INSERT INTO task_mutation_requests (request_id, operation, fingerprint, task_id) VALUES ('request', 'create', ?, 'FOO-0001')")
            .bind("a".repeat(64))
            .execute(&pool)
            .await
            .unwrap();
        for statement in [
            "UPDATE task_mutation_requests SET task_title = 'only title'",
            "UPDATE task_mutation_requests SET task_status = 'done'",
            "UPDATE task_mutation_requests SET task_title = 'task', task_status = 'invalid'",
        ] {
            assert!(
                sqlx::query(statement).execute(&pool).await.is_err(),
                "{statement}"
            );
        }
        sqlx::query(
            "UPDATE task_mutation_requests SET task_title = 'task', task_status = 'active'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            sqlx::query("UPDATE task_prompt_lanes SET marker = 'goal' WHERE lane = 'goals'")
                .execute(&pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query("UPDATE task_prompt_lanes SET header = 'Context' WHERE lane = 'goals'")
                .execute(&pool)
                .await
                .is_err()
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM task_prompt_lanes")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 4);
        check_database_ready(&pool).await.unwrap();
    }
}
