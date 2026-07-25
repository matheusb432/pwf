use sqlx::{Row, SqlitePool};
use tempfile::TempDir;

async fn migrated_pool(temp_directory: &TempDir) -> anyhow::Result<SqlitePool> {
    let database_path = temp_directory.path().join("pwf.sqlite3");
    let pool = pwf_infra::database::build_pool(&database_path).await?;
    pwf_infra::database::migrate_database(&pool).await?;
    Ok(pool)
}

#[test]
fn database_path_uses_the_pwf_data_directory() {
    assert_eq!(
        pwf_infra::database::database_path_in(std::path::Path::new("/data")),
        std::path::PathBuf::from("/data/pwf/pwf.sqlite3")
    );
}

#[tokio::test]
async fn migration_creates_project_tables_and_active_projects_view() -> anyhow::Result<()> {
    let temp_directory = tempfile::tempdir()?;
    let pool = migrated_pool(&temp_directory).await?;

    let objects = sqlx::query("SELECT name FROM sqlite_master WHERE name IN ('project_sources', 'projects', 'active_projects') ORDER BY name")
        .fetch_all(&pool)
        .await?;

    assert_eq!(
        objects
            .iter()
            .map(|row| row.get::<String, _>("name"))
            .collect::<Vec<_>>(),
        ["active_projects", "project_sources", "projects"]
    );
    Ok(())
}

#[tokio::test]
async fn pool_enables_foreign_keys_and_wal_journal_mode() -> anyhow::Result<()> {
    let temp_directory = tempfile::tempdir()?;
    let pool = migrated_pool(&temp_directory).await?;

    let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&pool)
        .await?;
    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&pool)
        .await?;

    assert_eq!(foreign_keys, 1);
    assert_eq!(journal_mode, "wal");
    Ok(())
}

#[tokio::test]
async fn schema_rejects_invalid_source_and_task_kinds() -> anyhow::Result<()> {
    let temp_directory = tempfile::tempdir()?;
    let pool = migrated_pool(&temp_directory).await?;

    let invalid_source =
        sqlx::query("INSERT INTO project_sources (kind, value) VALUES ('remote', '/repo')")
            .execute(&pool)
            .await;
    assert!(invalid_source.is_err());

    sqlx::query("INSERT INTO project_sources (kind, value) VALUES ('directory', '/repo')")
        .execute(&pool)
        .await?;
    let invalid_tasks = sqlx::query("INSERT INTO projects (id, project_source_id, title, tasks_kind, tasks_path) VALUES ('PWF', 1, 'pwf', 'remote', '/tasks')")
        .execute(&pool)
        .await;
    assert!(invalid_tasks.is_err());
    Ok(())
}

#[tokio::test]
async fn schema_rejects_duplicate_source_pairs() -> anyhow::Result<()> {
    let temp_directory = tempfile::tempdir()?;
    let pool = migrated_pool(&temp_directory).await?;

    sqlx::query("INSERT INTO project_sources (kind, value) VALUES ('directory', '/repo')")
        .execute(&pool)
        .await?;
    let duplicate =
        sqlx::query("INSERT INTO project_sources (kind, value) VALUES ('directory', '/repo')")
            .execute(&pool)
            .await;

    assert!(duplicate.is_err());
    Ok(())
}

#[tokio::test]
async fn schema_rejects_duplicate_task_locations() -> anyhow::Result<()> {
    let temp_directory = tempfile::tempdir()?;
    let pool = migrated_pool(&temp_directory).await?;

    sqlx::query("INSERT INTO project_sources (kind, value) VALUES ('directory', '/repo-one'), ('directory', '/repo-two')")
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO projects (id, project_source_id, title, tasks_kind, tasks_path) VALUES ('ONE', 1, 'one', 'directory', '/tasks')")
        .execute(&pool)
        .await?;
    let duplicate = sqlx::query("INSERT INTO projects (id, project_source_id, title, tasks_kind, tasks_path) VALUES ('TWO', 2, 'two', 'directory', '/tasks')")
        .execute(&pool)
        .await;

    assert!(duplicate.is_err());
    Ok(())
}

#[tokio::test]
async fn active_projects_excludes_paused_rows() -> anyhow::Result<()> {
    let temp_directory = tempfile::tempdir()?;
    let pool = migrated_pool(&temp_directory).await?;

    sqlx::query("INSERT INTO project_sources (kind, value) VALUES ('directory', '/repo')")
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO projects (id, project_source_id, title, tasks_kind, tasks_path) VALUES ('ACTIVE', 1, 'active', 'directory', '/active'), ('PAUSED', 1, 'paused', 'directory', '/paused')")
        .execute(&pool)
        .await?;
    sqlx::query("UPDATE projects SET paused_at = '2026-07-25T00:00:00.000Z' WHERE id = 'PAUSED'")
        .execute(&pool)
        .await?;

    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM active_projects ORDER BY id")
        .fetch_all(&pool)
        .await?;
    assert_eq!(ids, ["ACTIVE"]);
    Ok(())
}
