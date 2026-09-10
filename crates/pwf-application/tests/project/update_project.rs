use std::assert_matches;

use pwf_application::project::{update_project, update_project::UpdateProjectError};
use pwf_models::project::{ProjectSource, ProjectSourceKind, ProjectSourceValue};
use pwf_wire::project::UpdateProject;

use crate::support::insert_project;

fn update(project_id: &str, source_value: &str) -> UpdateProject {
    UpdateProject {
        obsidian_vault: pwf_wire::patch_field::PatchField::NoAction,
        snapshot_enabled: pwf_wire::set_field::SetField::NoAction,
        id: project_id.parse().unwrap(),
        source: pwf_wire::patch_field::PatchField::Set(ProjectSource::new(
            ProjectSourceKind::Directory,
            ProjectSourceValue::try_new(source_value).unwrap(),
        )),
    }
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn source_update_is_atomic_idempotent_and_reuses_shared_sources(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/old", "/tasks/foo", true).await;
    insert_project(&pool, "BAR", "bar", "/work/shared", "/tasks/bar", false).await;

    update_project::execute(update("FOO", "/work/shared"), &pool)
        .await
        .unwrap();
    update_project::execute(update("FOO", "/work/shared"), &pool)
        .await
        .unwrap();

    let stored: (String, String, String, String, bool) = sqlx::query_as(
        r"
        SELECT
            projects.id,
            projects.title,
            project_sources.value,
            projects.tasks_path,
            projects.paused_at IS NOT NULL
        FROM projects
        LEFT JOIN project_sources ON project_sources.id = projects.project_source_id
        WHERE projects.id = 'FOO'
        ",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        stored,
        (
            "FOO".to_string(),
            "foo".to_string(),
            "/work/shared".to_string(),
            "/tasks/foo".to_string(),
            true,
        )
    );
    let shared_source_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_sources WHERE value = '/work/shared'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(shared_source_count, 1);
    pool.close().await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn missing_project_rolls_back_the_candidate_source(pool: sqlx::SqlitePool) {
    let error = update_project::execute(update("MISS", "/work/candidate"), &pool)
        .await
        .unwrap_err();

    assert_matches!(
        error,
        UpdateProjectError::ProjectNotFound { id } if id.as_ref() == "MISS"
    );
    let source_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_sources WHERE value = '/work/candidate'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(source_count, 0);
    pool.close().await;
}
