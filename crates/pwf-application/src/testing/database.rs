use sqlx::SqlitePool;

pub(crate) static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../pwf-infra/migrations");

pub(crate) async fn insert_project(
    pool: &SqlitePool,
    id: &str,
    title: &str,
    source_value: &str,
    tasks_path: &str,
    is_paused: bool,
) {
    let source_id = sqlx::query(
        "INSERT INTO project_sources (kind, value, created_at)
         VALUES ('directory', ?, '2026-07-26T00:00:00.000Z')",
    )
    .bind(source_value)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_rowid();
    let paused_at = is_paused.then_some("2026-07-26T00:00:00.000Z");
    sqlx::query(
        "INSERT INTO projects (
            id,
            project_source_id,
            title,
            tasks_kind,
            tasks_path,
            created_at,
            paused_at
        )
        VALUES (?, ?, ?, 'directory', ?, '2026-07-26T00:00:00.000Z', ?)",
    )
    .bind(id)
    .bind(source_id)
    .bind(title)
    .bind(tasks_path)
    .bind(paused_at)
    .execute(pool)
    .await
    .unwrap();
}
