//! Dedicated migration application and read-only runtime readiness checks.

use std::{collections::BTreeMap, time::Duration};

use anyhow::Context as _;
use sqlx::SqlitePool;

pub(super) static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

const DATABASE_MIGRATION_WAIT_MAX: Duration = Duration::from_secs(10);
const DATABASE_READY_REMEDY: &str = "run `cargo run --quiet -p pwf-migrator` before running pwf";

#[derive(Debug, Clone, PartialEq, Eq)]
struct MigrationRecord {
    version: i64,
    success: bool,
    checksum: Vec<u8>,
}

pub async fn migrate_database(pool: &SqlitePool) -> anyhow::Result<()> {
    tokio::time::timeout(DATABASE_MIGRATION_WAIT_MAX, MIGRATOR.run(pool))
        .await
        .context("database migration window elapsed")?
        .context("running database migrations")
}

pub async fn check_database_ready(pool: &SqlitePool) -> anyhow::Result<()> {
    let table_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = '_sqlx_migrations')",
    )
    .fetch_one(pool)
    .await
    .context("checking the SQLx migration table")?;
    if !table_exists {
        return Err(database_not_ready("the SQLx migration table is missing"));
    }

    let applied = sqlx::query_as::<_, (i64, bool, Vec<u8>)>(
        "SELECT version, success, checksum FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(pool)
    .await
    .context("reading applied SQLx migrations")?
    .into_iter()
    .map(|(version, success, checksum)| MigrationRecord {
        version,
        success,
        checksum,
    })
    .collect::<Vec<_>>();
    let expected = MIGRATOR
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration())
        .map(|migration| MigrationRecord {
            version: migration.version,
            success: true,
            checksum: migration.checksum.to_vec(),
        })
        .collect::<Vec<_>>();

    validate_migrations(&expected, &applied).map_err(database_not_ready)
}

fn validate_migrations(
    expected: &[MigrationRecord],
    applied: &[MigrationRecord],
) -> Result<(), String> {
    let expected_by_version = expected
        .iter()
        .map(|migration| (migration.version, migration))
        .collect::<BTreeMap<_, _>>();
    let applied_by_version = applied
        .iter()
        .map(|migration| (migration.version, migration))
        .collect::<BTreeMap<_, _>>();

    for migration in applied {
        if !migration.success {
            return Err(format!("SQLx migration {} is dirty", migration.version));
        }
        if !expected_by_version.contains_key(&migration.version) {
            return Err(format!(
                "unknown SQLx migration {} is applied",
                migration.version
            ));
        }
    }
    for migration in expected {
        let Some(applied_migration) = applied_by_version.get(&migration.version) else {
            return Err(format!("SQLx migration {} is pending", migration.version));
        };
        if migration.checksum != applied_migration.checksum {
            return Err(format!(
                "SQLx migration {} checksum does not match",
                migration.version
            ));
        }
    }
    Ok(())
}

fn database_not_ready(reason: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!("database schema is not ready: {reason}; {DATABASE_READY_REMEDY}")
}

#[cfg(test)]
mod tests {
    use super::{MigrationRecord, check_database_ready, validate_migrations};

    #[tokio::test]
    async fn readiness_is_read_only_and_migration_is_idempotent() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();

        let error = check_database_ready(&pool).await.unwrap_err();
        assert!(error.to_string().contains("pwf-migrator"));

        let migration_table_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = '_sqlx_migrations')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!migration_table_exists);

        crate::database::migrate_database(&pool).await.unwrap();
        crate::database::migrate_database(&pool).await.unwrap();
        check_database_ready(&pool).await.unwrap();
    }

    #[test]
    fn migrations_accept_the_complete_matching_set() {
        let expected = [record(1, true, b"one"), record(2, true, b"two")];
        let applied = [record(1, true, b"one"), record(2, true, b"two")];

        assert_eq!(validate_migrations(&expected, &applied), Ok(()));
    }

    #[test]
    fn migrations_reject_a_pending_version() {
        let expected = [record(1, true, b"one"), record(2, true, b"two")];
        let applied = [record(1, true, b"one")];

        assert_eq!(
            validate_migrations(&expected, &applied),
            Err("SQLx migration 2 is pending".to_owned())
        );
    }

    #[test]
    fn migrations_reject_a_dirty_version() {
        assert_eq!(
            validate_migrations(&[record(1, true, b"one")], &[record(1, false, b"one")]),
            Err("SQLx migration 1 is dirty".to_owned())
        );
    }

    #[test]
    fn migrations_reject_a_checksum_mismatch() {
        assert_eq!(
            validate_migrations(
                &[record(1, true, b"expected")],
                &[record(1, true, b"actual")]
            ),
            Err("SQLx migration 1 checksum does not match".to_owned())
        );
    }

    #[test]
    fn migrations_reject_an_unknown_applied_version() {
        let expected = [record(1, true, b"one")];
        let applied = [record(1, true, b"one"), record(99, true, b"unknown")];

        assert_eq!(
            validate_migrations(&expected, &applied),
            Err("unknown SQLx migration 99 is applied".to_owned())
        );
    }

    fn record(version: i64, success: bool, checksum: &[u8]) -> MigrationRecord {
        MigrationRecord {
            version,
            success,
            checksum: checksum.to_vec(),
        }
    }
}
