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

    SqlitePoolOptions::new()
        .max_connections(connections_max)
        .acquire_timeout(DATABASE_WAIT_MAX)
        .connect_with(options)
        .await
        .with_context(|| format!("connecting to database {}", path.display()))
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;

    use super::{
        build_migration_pool, build_pool, build_read_only_pool, check_database_ready,
        migrate_database, migrations,
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

    const MIGRATION_VERSION_0001: i64 = 20_260_725_000_000;
    const MIGRATION_VERSION_0002: i64 = 20_260_801_000_000;
    const MIGRATION_VERSION_0003: i64 = 20_260_806_000_000;
    const MIGRATION_VERSION_0004: i64 = 20_260_830_000_000;
    const MIGRATION_VERSION_0005: i64 = 20_260_831_001_753;
    const MIGRATION_VERSION_0006: i64 = 20_260_903_000_000;

    #[tokio::test]
    async fn migration_0005_to_current_preserves_state_and_removes_review_results() {
        let directory = tempfile::tempdir().unwrap();
        let pool = build_migration_pool(&directory.path().join("pwf.sqlite3"))
            .await
            .unwrap();
        migrations::MIGRATOR
            .run_to(MIGRATION_VERSION_0005, &pool)
            .await
            .unwrap();

        let project_source_id = insert_migration_0005_fixture(&pool).await;

        let readiness_error = check_database_ready(&pool).await.unwrap_err();
        assert!(
            readiness_error
                .to_string()
                .contains("SQLx migration 20260903000000 is pending")
        );

        migrate_database(&pool).await.unwrap();

        assert_project_state_preserved(&pool, project_source_id).await;
        assert_current_migration_integrity(&pool).await;
    }

    async fn insert_migration_0005_fixture(pool: &SqlitePool) -> i64 {
        let project_source_id = sqlx::query(
                "INSERT INTO project_sources (kind, value, created_at) VALUES ('directory', '/work/foo', '2026-07-25T12:00:00.000Z')",
            )
            .execute(pool)
            .await
            .unwrap()
            .last_insert_rowid();
        sqlx::query(
                "INSERT INTO projects (id, project_source_id, title, tasks_kind, tasks_path, created_at, paused_at)
                 VALUES ('FOO', ?, 'foo', 'directory', '/tasks/foo', '2026-07-25T12:01:00.000Z', '2026-07-26T09:30:00.000Z')",
            )
            .bind(project_source_id)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO task_mutation_requests (
                request_id,
                operation,
                fingerprint,
                task_id,
                state,
                outcome,
                result_task_id,
                created_at,
                completed_at
            ) VALUES (
                'migration-fixture',
                'complete',
                'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                'FOO-0001',
                'completed',
                'completed',
                'FOO-0002',
                '2026-08-30T12:00:00.000Z',
                '2026-08-30T12:00:01.000Z'
            )",
        )
        .execute(pool)
        .await
        .unwrap();

        project_source_id
    }

    async fn assert_project_state_preserved(pool: &SqlitePool, project_source_id: i64) {
        let project: (String, i64, String, String, String, String, Option<String>) =
            sqlx::query_as(
                "SELECT id, project_source_id, title, tasks_kind, tasks_path, created_at, paused_at
                     FROM projects WHERE id = 'FOO'",
            )
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(
            project,
            (
                "FOO".to_owned(),
                project_source_id,
                "foo".to_owned(),
                "directory".to_owned(),
                "/tasks/foo".to_owned(),
                "2026-07-25T12:01:00.000Z".to_owned(),
                Some("2026-07-26T09:30:00.000Z".to_owned()),
            )
        );
    }

    async fn assert_current_migration_integrity(pool: &SqlitePool) {
        let active_project_ids =
            sqlx::query_scalar::<_, String>("SELECT id FROM active_projects ORDER BY id")
                .fetch_all(pool)
                .await
                .unwrap();
        assert!(active_project_ids.is_empty());

        let foreign_key_violations = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(pool)
            .await
            .unwrap();
        assert!(foreign_key_violations.is_empty());

        let applied_versions =
            sqlx::query_scalar::<_, i64>("SELECT version FROM _sqlx_migrations ORDER BY version")
                .fetch_all(pool)
                .await
                .unwrap();
        assert_eq!(
            applied_versions,
            vec![
                MIGRATION_VERSION_0001,
                MIGRATION_VERSION_0002,
                MIGRATION_VERSION_0003,
                MIGRATION_VERSION_0004,
                MIGRATION_VERSION_0005,
                MIGRATION_VERSION_0006,
            ]
        );
        let request_table_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'task_mutation_requests')",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert!(request_table_exists);
        assert_task_mutation_request_migrated(pool).await;
        let prompt_lanes = sqlx::query_as::<_, (String, String, String)>(
            "SELECT lane, marker, header FROM task_prompt_lanes ORDER BY CASE lane
                WHEN 'goals' THEN 0
                WHEN 'context' THEN 1
                WHEN 'constraints' THEN 2
                WHEN 'done_when' THEN 3
                ELSE 4
            END",
        )
        .fetch_all(pool)
        .await
        .unwrap();
        assert_eq!(
            prompt_lanes,
            vec![
                ("goals".to_string(), "/g".to_string(), "Goals".to_string()),
                (
                    "context".to_string(),
                    "/c".to_string(),
                    "Context".to_string(),
                ),
                (
                    "constraints".to_string(),
                    "/n".to_string(),
                    "Constraints".to_string(),
                ),
                (
                    "done_when".to_string(),
                    "/d".to_string(),
                    "Done When".to_string(),
                ),
            ]
        );
        assert!(
            sqlx::query("UPDATE task_prompt_lanes SET marker = 'goal' WHERE lane = 'goals'")
                .execute(pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query("UPDATE task_prompt_lanes SET header = 'Context' WHERE lane = 'goals'",)
                .execute(pool)
                .await
                .is_err()
        );
        check_database_ready(pool).await.unwrap();
    }

    async fn assert_task_mutation_request_migrated(pool: &SqlitePool) {
        let preserved_request: (String, String, String, String) = sqlx::query_as(
            "SELECT request_id, operation, task_id, outcome FROM task_mutation_requests
             WHERE request_id = 'migration-fixture'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(
            preserved_request,
            (
                "migration-fixture".to_string(),
                "complete".to_string(),
                "FOO-0001".to_string(),
                "completed".to_string(),
            )
        );
        let request_columns = sqlx::query_scalar::<_, String>(
            "SELECT name FROM pragma_table_info('task_mutation_requests') ORDER BY cid",
        )
        .fetch_all(pool)
        .await
        .unwrap();
        assert!(!request_columns.iter().any(|name| name == "result_task_id"));
    }
}
