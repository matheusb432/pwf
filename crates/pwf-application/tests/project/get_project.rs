use pwf_application::project::{get_project, get_project::*};
use pwf_models::project::ProjectId;
use pwf_wire::project::{GetProject, ProjectStatusFilter};

use crate::support::insert_project;

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn status_filter_controls_paused_project_visibility(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", true).await;
    let id = ProjectId::try_new("FOO").unwrap();

    let active = get_project::execute(
        GetProject {
            id: id.clone(),
            status: ProjectStatusFilter::ActiveOnly,
        },
        &pool,
    )
    .await;
    let all = get_project::execute(
        GetProject {
            id,
            status: ProjectStatusFilter::IncludingPaused,
        },
        &pool,
    )
    .await
    .unwrap();

    assert!(matches!(
        active,
        Err(GetProjectError::ProjectNotFound { .. })
    ));
    assert!(all.is_paused);
}
