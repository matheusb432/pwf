use pwf_application::ports::user_settings::UserSettingsReader as _;
use pwf_infra::{
    database::{self, MigrationCompatibility},
    user_settings::TomlSettingsStore,
};
use pwf_local_transport::LocalEndpoint;
use pwf_wire::doctor::{Check, DoctorReport};

/// Inspects startup prerequisites without creating state, binding IPC or applying migrations.
pub async fn inspect() -> DoctorReport {
    let mut checks = Vec::new();
    checks.push(match TomlSettingsStore::from_environment().load() {
        Ok(_) => Check::pass("settings", "User settings are valid"),
        Err(error) => Check::fail(
            "settings",
            error.to_string(),
            "Correct the settings file. Task colors belong under [colors.task], project colors under [colors.project], and note colors under [colors.note].",
        ),
    });
    match LocalEndpoint::from_environment() {
        Ok(endpoint) => checks.push(Check::pass(
            "endpoint",
            endpoint.path().display().to_string(),
        )),
        Err(error) => checks.push(Check::fail(
            "endpoint",
            error.to_string(),
            "Correct PWF_RUNTIME_DIR and the runtime directory permissions.",
        )),
    }
    match database::database_path() {
        Ok(path) => {
            checks.push(Check::pass("database", path.display().to_string()));
            checks.push(inspect_database(&path).await);
        }
        Err(error) => checks.push(Check::fail(
            "database",
            format!("{error:#}"),
            "Correct PWF_DATABASE_PATH or the platform data directory.",
        )),
    }
    DoctorReport {
        version: env!("CARGO_PKG_VERSION").into(),
        checks,
    }
}

async fn inspect_database(path: &std::path::Path) -> Check {
    match path.try_exists() {
        Ok(false) => {
            return Check::warning(
                "migrations",
                "Database has not been created",
                "Start the server to initialize the database.",
            );
        }
        Err(error) => {
            return Check::fail(
                "database access",
                error.to_string(),
                "Check the database path and directory permissions.",
            );
        }
        Ok(true) => {}
    }
    let pool = match database::build_read_only_pool(path).await {
        Ok(pool) => pool,
        Err(error) => {
            return Check::fail(
                "database access",
                format!("{error:#}"),
                "Check database permissions and whether another process holds a lock.",
            );
        }
    };
    let result = database::check_database_compatible(&pool).await;
    pool.close().await;
    match result {
        Ok(MigrationCompatibility::Compatible { pending: 0 }) => Check::pass(
            "migrations",
            "Applied migrations match this server's embedded catalog",
        ),
        Ok(MigrationCompatibility::Compatible { pending }) => Check::warning(
            "migrations",
            format!("{pending} migration(s) pending"),
            "Start the server to apply pending migrations.",
        ),
        Ok(MigrationCompatibility::Incompatible { reason }) => Check::fail(
            "migrations",
            reason,
            "Use a server release compatible with this database's migration history, or a reviewed migration recovery. Do not delete the database or rewrite its migration ledger.",
        ),
        Err(error) => Check::fail(
            "database access",
            format!("{error:#}"),
            "Check database permissions, its migration table and whether another process holds a lock.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use pwf_wire::doctor::CheckStatus;

    use super::*;

    #[tokio::test]
    async fn offline_check_rejects_incompatible_history_without_repairing_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("pwf.sqlite3");
        let pool = database::build_migration_pool(&path).await.unwrap();
        database::migrate_database(&pool).await.unwrap();
        sqlx::query("UPDATE _sqlx_migrations SET checksum = X'00'")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        let before = std::fs::read(&path).unwrap();
        let check = inspect_database(&path).await;
        assert_eq!(check.status, CheckStatus::Fail);
        assert!(check.detail.contains("migration 0 checksum"));
        assert!(check.action.as_deref().unwrap().contains("compatible"));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[tokio::test]
    async fn offline_check_accepts_pending_migrations_without_applying_them() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("pwf.sqlite3");
        let pool = database::build_pool(&path).await.unwrap();
        pool.close().await;
        let before = std::fs::read(&path).unwrap();
        let check = inspect_database(&path).await;
        assert_eq!(check.status, CheckStatus::Warning);
        assert!(check.detail.contains("pending"));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
