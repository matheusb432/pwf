use std::path::PathBuf;

use pwf_application::project::resume_project;
use pwf_models::project::{HomeDirectory, ProjectId};

use crate::support::insert_project;

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn runtime_alias_of_other_paused_project_is_rejected(pool: sqlx::SqlitePool) {
    let home_path = PathBuf::from("/home/tester");
    let home = HomeDirectory::new(home_path.clone());
    let resolved_path = home_path.join("tasks/shared");
    insert_project(&pool, "FOO", "foo", "/work/foo", "~/tasks/shared", true).await;
    insert_project(
        &pool,
        "ALT",
        "alt",
        "/work/ALT",
        &resolved_path.to_string_lossy(),
        true,
    )
    .await;

    let error = resume_project::execute(ProjectId::try_new("FOO").unwrap(), &pool, &home)
        .await
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        format!(
            "managed projects ALT and FOO resolve to the same task location: {}",
            resolved_path.display()
        )
    );
    let paused_at: Option<String> =
        sqlx::query_scalar("SELECT paused_at FROM projects WHERE id = 'FOO'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(paused_at.is_some());
    pool.close().await;
}
