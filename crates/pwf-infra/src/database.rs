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

pub use migrations::{check_database_ready, migrate_database};

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

    SqlitePoolOptions::new()
        .max_connections(connections_max)
        .acquire_timeout(DATABASE_WAIT_MAX)
        .connect_with(options)
        .await
        .with_context(|| format!("connecting to database {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{
        build_migration_pool, build_pool, check_database_ready, migrate_database, migrations,
    };

    #[tokio::test]
    async fn pools_enable_foreign_keys_and_persist_wal_journal_mode() {
        let directory = tempfile::tempdir().expect("create database directory");
        let path = directory.path().join("pwf.sqlite3");
        let migration_pool = build_migration_pool(&path)
            .await
            .expect("build SQLite migration pool");
        migration_pool.close().await;
        let pool = build_pool(&path).await.expect("build SQLite pool");

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

    #[tokio::test]
    async fn migrations_enforce_project_identity() {
        let directory = tempfile::tempdir().expect("create database directory");
        let pool = build_migration_pool(&directory.path().join("pwf.sqlite3"))
            .await
            .expect("build SQLite migration pool");
        migrate_database(&pool).await.expect("migrate database");

        let source_id = sqlx::query(
            "INSERT INTO project_sources (kind, value) VALUES ('directory', '/work/pwf')",
        )
        .execute(&pool)
        .await
        .expect("insert project source")
        .last_insert_rowid();

        sqlx::query(
            "INSERT INTO projects (id, project_source_id, title, tasks_kind, tasks_path)
             VALUES ('PWF', ?, 'pwf', 'directory', '/tasks/pwf')",
        )
        .bind(source_id)
        .execute(&pool)
        .await
        .expect("insert canonical project");

        for (id, title, tasks_path) in [
            ("PW", "short", "/tasks/short"),
            ("TOOL", "long", "/tasks/long"),
            ("alt", "lowercase", "/tasks/lowercase"),
        ] {
            let invalid_id = sqlx::query(
                "INSERT INTO projects (id, project_source_id, title, tasks_kind, tasks_path)
                 VALUES (?, ?, ?, 'directory', ?)",
            )
            .bind(id)
            .bind(source_id)
            .bind(title)
            .bind(tasks_path)
            .execute(&pool)
            .await;
            assert!(invalid_id.is_err(), "accepted invalid project ID {id}");
        }

        let duplicate_title = sqlx::query(
            "INSERT INTO projects (id, project_source_id, title, tasks_kind, tasks_path)
             VALUES ('ALT', ?, 'PWF', 'directory', '/tasks/alt')",
        )
        .bind(source_id)
        .execute(&pool)
        .await;
        assert!(duplicate_title.is_err());
    }

    #[tokio::test]
    async fn migration_preserves_existing_project_state() {
        let directory = tempfile::tempdir().expect("create database directory");
        let pool = build_migration_pool(&directory.path().join("pwf.sqlite3"))
            .await
            .expect("build SQLite migration pool");
        migrations::MIGRATOR
            .run_to(20_260_725_000_000, &pool)
            .await
            .expect("apply initial migration");

        let source_id = sqlx::query(
            "INSERT INTO project_sources (kind, value, created_at) VALUES ('directory', '/work/pwf', '2026-07-25T12:00:00.000Z')",
        )
        .execute(&pool)
        .await
        .expect("insert project source")
        .last_insert_rowid();
        sqlx::query(
            "INSERT INTO projects (id, project_source_id, title, tasks_kind, tasks_path, created_at, paused_at)
             VALUES ('PWF', ?, 'pwf', 'directory', '/tasks/pwf', '2026-07-25T12:01:00.000Z', '2026-07-26T09:30:00.000Z')",
        )
        .bind(source_id)
        .execute(&pool)
        .await
        .expect("insert project");

        let readiness_error = check_database_ready(&pool)
            .await
            .expect_err("initial schema must report the pending migration");
        assert!(
            readiness_error
                .to_string()
                .contains("SQLx migration 20260801000000 is pending")
        );

        migrate_database(&pool).await.expect("migrate database");

        let project: (String, i64, String, String, String, String, Option<String>) =
            sqlx::query_as(
                "SELECT id, project_source_id, title, tasks_kind, tasks_path, created_at, paused_at
                 FROM projects WHERE id = 'PWF'",
            )
            .fetch_one(&pool)
            .await
            .expect("read migrated project");
        assert_eq!(
            project,
            (
                "PWF".to_owned(),
                source_id,
                "pwf".to_owned(),
                "directory".to_owned(),
                "/tasks/pwf".to_owned(),
                "2026-07-25T12:01:00.000Z".to_owned(),
                Some("2026-07-26T09:30:00.000Z".to_owned()),
            )
        );
    }
}
