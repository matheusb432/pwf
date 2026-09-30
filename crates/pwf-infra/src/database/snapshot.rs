use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context as _, ensure};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

use super::{DATABASE_WAIT_MAX, migrate_database, migrations::applied_schema_version};

const SNAPSHOT_VACUUM_WAIT_MAX: Duration = Duration::from_secs(60);
const DATABASE_COMPANION_SUFFIXES: [&str; 3] = ["-wal", "-shm", "-journal"];

pub async fn export_snapshot(database_path: &Path, snapshot_path: &Path) -> anyhow::Result<i64> {
    ensure!(
        database_path
            .try_exists()
            .with_context(|| format!("checking pwf database {}", database_path.display()))?,
        "pwf database does not exist: {}",
        database_path.display()
    );
    let snapshot_path_text = snapshot_path.to_str().with_context(|| {
        format!(
            "snapshot path is not valid UTF-8: {}",
            snapshot_path.display()
        )
    })?;

    let live = SqlitePoolOptions::new()
        .max_connections(1)
        .acquire_timeout(DATABASE_WAIT_MAX)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(database_path)
                .read_only(true)
                .busy_timeout(DATABASE_WAIT_MAX),
        )
        .await
        .with_context(|| format!("opening database read-only at {}", database_path.display()))?;
    let vacuum = tokio::time::timeout(
        SNAPSHOT_VACUUM_WAIT_MAX,
        sqlx::query("VACUUM INTO ?")
            .bind(snapshot_path_text)
            .execute(&live),
    )
    .await;
    live.close().await;
    vacuum
        .context("database snapshot window elapsed")?
        .with_context(|| format!("writing database snapshot to {}", snapshot_path.display()))?;

    let snapshot = single_file_pool(snapshot_path).await?;
    let version = applied_schema_version(&snapshot).await;
    snapshot.close().await;
    version?.with_context(|| {
        format!(
            "database snapshot has no applied migrations: {}",
            snapshot_path.display()
        )
    })
}

pub async fn stage_snapshot(
    snapshot_path: &Path,
    staged_path: &Path,
) -> anyhow::Result<(i64, i64)> {
    copy_new(snapshot_path, staged_path).with_context(|| {
        format!(
            "copying database snapshot {} to {}",
            snapshot_path.display(),
            staged_path.display()
        )
    })?;
    let staged = single_file_pool(staged_path).await?;
    let result = migrate_staged(&staged, staged_path).await;
    staged.close().await;
    result
}

pub fn replace_database_file(staged_path: &Path, database_path: &Path) -> anyhow::Result<()> {
    for suffix in DATABASE_COMPANION_SUFFIXES {
        let companion = companion_path(database_path, suffix);
        match fs::remove_file(&companion) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("removing stale database companion {}", companion.display())
                });
            }
        }
    }
    fs::rename(staged_path, database_path).with_context(|| {
        format!(
            "moving staged database {} into place at {}",
            staged_path.display(),
            database_path.display()
        )
    })
}

async fn migrate_staged(staged: &SqlitePool, staged_path: &Path) -> anyhow::Result<(i64, i64)> {
    let snapshot_schema_version = applied_schema_version(staged).await?.with_context(|| {
        format!(
            "database snapshot has no applied migrations: {}",
            staged_path.display()
        )
    })?;
    migrate_database(staged).await.context(
        "snapshot migration history is newer than or diverges from this build's catalog; \
         install a compatible pwf-server release",
    )?;
    let report: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(staged)
        .await
        .context("checking staged database integrity")?;
    ensure!(
        report == ["ok"],
        "staged database failed its integrity check: {}",
        report.join("; ")
    );
    let schema_version = applied_schema_version(staged)
        .await?
        .context("staged database has no applied migrations after migrating")?;
    Ok((snapshot_schema_version, schema_version))
}

async fn single_file_pool(path: &Path) -> anyhow::Result<SqlitePool> {
    SqlitePoolOptions::new()
        .max_connections(1)
        .acquire_timeout(DATABASE_WAIT_MAX)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .journal_mode(SqliteJournalMode::Delete)
                .foreign_keys(true)
                .busy_timeout(DATABASE_WAIT_MAX),
        )
        .await
        .with_context(|| format!("opening database {}", path.display()))
}

fn copy_new(source: &Path, destination: &Path) -> io::Result<()> {
    let mut source_file = File::open(source)?;
    let mut destination_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    io::copy(&mut source_file, &mut destination_file)?;
    destination_file.sync_all()
}

fn companion_path(database_path: &Path, suffix: &str) -> PathBuf {
    let mut path = database_path.as_os_str().to_os_string();
    path.push(suffix);
    path.into()
}
