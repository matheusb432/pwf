//! Embedded migrations with serialized execution and exact ledger validation.

use std::time::Duration;

use anyhow::{Context as _, ensure};
use sqlx::{SqliteConnection, SqlitePool};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();
const DATABASE_MIGRATION_WAIT_MAX: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct MigrationRecord {
    version: i64,
    description: String,
    success: bool,
    checksum: Vec<u8>,
}

pub async fn migrate_database(pool: &SqlitePool) -> anyhow::Result<()> {
    tokio::time::timeout(DATABASE_MIGRATION_WAIT_MAX, async {
        // SQLx's SQLite migration lock is a no-op; acquire the writer before reading the ledger.
        let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
        let applied = read_migrations(&mut transaction).await?;
        validate_prefix(&expected_migrations(), &applied)?;
        MIGRATOR.run(&mut *transaction).await?;
        let violations = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *transaction)
            .await?;
        ensure!(
            violations.is_empty(),
            "database migration violated foreign keys"
        );
        transaction.commit().await?;
        anyhow::Ok(())
    })
    .await
    .context("database migration window elapsed")?
    .context("running database migrations")
}

pub async fn check_database_ready(pool: &SqlitePool) -> anyhow::Result<()> {
    let applied = read_migrations(&mut *pool.acquire().await?).await?;
    let expected = expected_migrations();
    validate_prefix(&expected, &applied)?;
    ensure!(
        applied.len() == expected.len(),
        "database schema is not ready: migrations are pending"
    );
    Ok(())
}

async fn read_migrations(
    connection: &mut SqliteConnection,
) -> anyhow::Result<Vec<MigrationRecord>> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = '_sqlx_migrations')",
    ).fetch_one(&mut *connection).await?;
    if !exists {
        return Ok(Vec::new());
    }
    Ok(sqlx::query_as(
        "SELECT version, description, success, checksum FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(connection)
    .await?)
}

fn expected_migrations() -> Vec<MigrationRecord> {
    MIGRATOR
        .iter()
        .map(|migration| MigrationRecord {
            version: migration.version,
            description: migration.description.to_string(),
            success: true,
            checksum: migration.checksum.to_vec(),
        })
        .collect()
}

fn validate_prefix(
    expected: &[MigrationRecord],
    applied: &[MigrationRecord],
) -> anyhow::Result<()> {
    for (index, actual) in applied.iter().enumerate() {
        ensure!(actual.success, "SQLx migration {} is dirty", actual.version);
        let expected = expected
            .get(index)
            .context("unknown SQLx migration is applied")?;
        ensure!(
            actual.version == expected.version,
            "SQLx migration {} is not the expected version {}",
            actual.version,
            expected.version
        );
        ensure!(
            actual.description == expected.description,
            "SQLx migration {} description does not match",
            actual.version
        );
        ensure!(
            actual.checksum == expected.checksum,
            "SQLx migration {} checksum does not match",
            actual.version
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn readiness_is_read_only_and_migration_is_idempotent() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        assert!(check_database_ready(&pool).await.is_err());
        let tables: i64 =
            sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE type = 'table'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(tables, 0);
        migrate_database(&pool).await.unwrap();
        migrate_database(&pool).await.unwrap();
        check_database_ready(&pool).await.unwrap();
        let versions =
            sqlx::query_scalar::<_, i64>("SELECT version FROM _sqlx_migrations ORDER BY version")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            versions,
            MIGRATOR
                .iter()
                .map(|migration| migration.version)
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn summary_migration_preserves_legacy_receipts_and_checks_summary_pairs() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let initial = sqlx::migrate::Migrator {
            migrations: std::borrow::Cow::Owned(MIGRATOR.iter().take(1).cloned().collect()),
            ..sqlx::migrate::Migrator::DEFAULT
        };
        initial.run(&pool).await.unwrap();
        sqlx::query("INSERT INTO task_mutation_requests (request_id, operation, fingerprint, task_id, state, outcome, completed_at) VALUES ('legacy-delete', 'delete', ?, 'FOO-0001', 'completed', 'deleted', '2026-09-07T00:00:00Z')")
            .bind("a".repeat(64)).execute(&pool).await.unwrap();
        migrate_database(&pool).await.unwrap();
        let receipt: (String, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT outcome, task_title, task_status FROM task_mutation_requests WHERE request_id = 'legacy-delete'"
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(receipt, ("deleted".into(), None, None));
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
        sqlx::query("UPDATE task_mutation_requests SET task_title = 'deleted task', task_status = 'cancelled'").execute(&pool).await.unwrap();
        check_database_ready(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn vault_migration_preserves_projects_and_adds_nullable_configuration() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let previous = sqlx::migrate::Migrator {
            migrations: std::borrow::Cow::Owned(MIGRATOR.iter().take(2).cloned().collect()),
            ..sqlx::migrate::Migrator::DEFAULT
        };
        previous.run(&pool).await.unwrap();
        sqlx::query("INSERT INTO project_sources (kind, value) VALUES ('directory', '/work/foo')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO projects (id, project_source_id, title, tasks_kind, tasks_path) VALUES ('FOO', 1, 'foo', 'directory', '/notes/foo')").execute(&pool).await.unwrap();
        migrate_database(&pool).await.unwrap();
        let project: (String, Option<String>) = sqlx::query_as(
            "SELECT tasks_path, obsidian_vault FROM active_projects WHERE id = 'FOO'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(project, ("/notes/foo".into(), None));
        assert!(
            sqlx::query("UPDATE projects SET obsidian_vault = ' '")
                .execute(&pool)
                .await
                .is_err()
        );
        sqlx::query("UPDATE projects SET obsidian_vault = '~/notes'")
            .execute(&pool)
            .await
            .unwrap();
        let vault: String =
            sqlx::query_scalar("SELECT obsidian_vault FROM active_projects WHERE id = 'FOO'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(vault, "~/notes");
    }

    #[tokio::test]
    async fn nullable_source_migration_preserves_project_configuration() {
        type Row = (
            String,
            Option<i64>,
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
        );
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let previous = sqlx::migrate::Migrator {
            migrations: std::borrow::Cow::Owned(MIGRATOR.iter().take(3).cloned().collect()),
            ..sqlx::migrate::Migrator::DEFAULT
        };
        previous.run(&pool).await.unwrap();
        sqlx::query("INSERT INTO project_sources (kind, value) VALUES ('directory', '/work/foo')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO projects (id, project_source_id, title, tasks_kind, tasks_path, created_at, paused_at, obsidian_vault) VALUES ('FOO', 1, 'foo', 'directory', '/notes/foo', '2026-09-07T00:00:00Z', '2026-09-07T01:00:00Z', '/notes')").execute(&pool).await.unwrap();
        let query = "SELECT id, project_source_id, title, tasks_kind, tasks_path, created_at, paused_at, obsidian_vault FROM projects";
        let before: Row = sqlx::query_as(query).fetch_one(&pool).await.unwrap();
        migrate_database(&pool).await.unwrap();
        let after: Row = sqlx::query_as(query).fetch_one(&pool).await.unwrap();
        assert_eq!(after, before);
        sqlx::query("UPDATE projects SET project_source_id = NULL, paused_at = NULL")
            .execute(&pool)
            .await
            .unwrap();
        let source: Option<i64> =
            sqlx::query_scalar("SELECT project_source_id FROM active_projects")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(source, None);
        assert!(
            sqlx::query("UPDATE projects SET project_source_id = 999")
                .execute(&pool)
                .await
                .is_err()
        );
        pool.close().await;
    }

    #[tokio::test]
    async fn invalid_ledger_is_rejected_without_repair() {
        for statement in [
            "UPDATE _sqlx_migrations SET version = 99 WHERE version = 0",
            "UPDATE _sqlx_migrations SET success = false",
            "UPDATE _sqlx_migrations SET checksum = X'00'",
            "UPDATE _sqlx_migrations SET description = 'changed'",
        ] {
            let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
            migrate_database(&pool).await.unwrap();
            sqlx::query(statement).execute(&pool).await.unwrap();
            let before = read_migrations(&mut pool.acquire().await.unwrap())
                .await
                .unwrap();
            assert!(migrate_database(&pool).await.is_err());
            assert!(check_database_ready(&pool).await.is_err());
            let after = read_migrations(&mut pool.acquire().await.unwrap())
                .await
                .unwrap();
            assert_eq!(after, before);
        }
    }

    #[test]
    fn prefix_rejects_holes_duplicates_and_reordering() {
        let record = |version| MigrationRecord {
            version,
            description: "migration".into(),
            success: true,
            checksum: vec![1],
        };
        let expected = [record(0), record(1), record(2)];
        assert!(validate_prefix(&expected, &[record(0)]).is_ok());
        assert!(validate_prefix(&expected, &[record(0), record(2)]).is_err());
        assert!(validate_prefix(&expected, &[record(0), record(0)]).is_err());
        assert!(validate_prefix(&expected, &[record(1), record(0)]).is_err());
    }

    #[tokio::test]
    async fn failed_bootstrap_rolls_back_schema_and_ledger() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE projects (marker TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        assert!(migrate_database(&pool).await.is_err());
        let tables = sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(tables, ["projects"]);
    }
}
