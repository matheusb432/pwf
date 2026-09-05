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
