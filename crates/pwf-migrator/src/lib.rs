//! Embedded database setup for the local server.

use std::path::Path;

/// Applies and verifies the catalog before the server opens its runtime pool.
pub async fn run(path: &Path) -> anyhow::Result<()> {
    let pool = pwf_infra::database::build_migration_pool(path).await?;
    let result = async {
        pwf_infra::database::migrate_database(&pool).await?;
        pwf_infra::database::check_database_ready(&pool).await
    }
    .await;
    pool.close().await;
    result
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn upgrade_preserves_registry_rows_drops_receipts_and_keeps_snapshot_choices() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("pwf.sqlite3");
        let pool = pwf_infra::database::build_migration_pool(&path)
            .await
            .unwrap();
        sqlx::migrate!("../pwf-infra/migrations")
            .run_to(0, &pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO project_sources (id, kind, value, created_at) VALUES (41, 'directory', '/work/foo', '2026-07-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, project_source_id, title, tasks_kind, tasks_path, created_at, paused_at, obsidian_vault) VALUES ('FOO', 41, 'foo', 'directory', '/tasks/foo', '2026-07-02T00:00:00Z', '2026-08-01T00:00:00Z', '/notes'), ('BAR', NULL, 'bar', 'directory', '/tasks/bar', '2026-07-03T00:00:00Z', NULL, NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO task_mutation_requests (request_id, operation, fingerprint, task_id) VALUES ('request', 'create', ?, 'FOO-0001')",
        )
        .bind("a".repeat(64))
        .execute(&pool)
        .await
        .unwrap();
        let projects_query = "SELECT json_array(id, project_source_id, title, tasks_kind, tasks_path, created_at, paused_at, obsidian_vault) FROM projects ORDER BY id";
        let projects_before = sqlx::query_scalar::<_, String>(projects_query)
            .fetch_all(&pool)
            .await
            .unwrap();
        let sources_query =
            "SELECT json_array(id, kind, value, created_at) FROM project_sources ORDER BY id";
        let sources_before = sqlx::query_scalar::<_, String>(sources_query)
            .fetch_all(&pool)
            .await
            .unwrap();
        pool.close().await;

        let (first, second) = tokio::join!(super::run(&path), super::run(&path));
        first.unwrap();
        second.unwrap();
        let pool = pwf_infra::database::build_pool(&path).await.unwrap();
        let projects_after = sqlx::query_scalar::<_, String>(projects_query)
            .fetch_all(&pool)
            .await
            .unwrap();
        let sources_after = sqlx::query_scalar::<_, String>(sources_query)
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(projects_after, projects_before);
        assert_eq!(sources_after, sources_before);
        let snapshots =
            sqlx::query_scalar::<_, bool>("SELECT snapshot_enabled FROM projects ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(snapshots, [false, false]);
        let receipts_exist: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = 'task_mutation_requests')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!receipts_exist);
        sqlx::query("UPDATE projects SET snapshot_enabled = 1 WHERE id = 'FOO'")
            .execute(&pool)
            .await
            .unwrap();
        super::run(&path).await.unwrap();
        let snapshots =
            sqlx::query_scalar::<_, bool>("SELECT snapshot_enabled FROM projects ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(snapshots, [false, true]);
        pwf_infra::database::check_database_ready(&pool)
            .await
            .unwrap();
        pool.close().await;
    }

    #[tokio::test]
    async fn concurrent_startup_preserves_existing_rows() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("pwf.sqlite3");
        let (first, second) = tokio::join!(super::run(&path), super::run(&path));
        first.unwrap();
        second.unwrap();
        let pool = pwf_infra::database::build_pool(&path).await.unwrap();
        sqlx::query("UPDATE task_prompt_lanes SET header = 'Objectives' WHERE lane = 'goals'")
            .execute(&pool)
            .await
            .unwrap();
        super::run(&path).await.unwrap();
        let header: String =
            sqlx::query_scalar("SELECT header FROM task_prompt_lanes WHERE lane = 'goals'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(header, "Objectives");
        pwf_infra::database::check_database_ready(&pool)
            .await
            .unwrap();
        pool.close().await;
    }
}
