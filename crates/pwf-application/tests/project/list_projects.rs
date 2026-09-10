use pwf_application::project::list_projects;
use pwf_wire::project::ProjectStatusFilter;

use crate::support::insert_project;

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn active_list_filters_paused_projects_and_sorts_by_title(pool: sqlx::SqlitePool) {
    insert_project(&pool, "ZED", "zeta", "/work/zeta", "/tasks/zeta", false).await;
    insert_project(&pool, "ALP", "alpha", "/work/alpha", "/tasks/alpha", false).await;
    insert_project(&pool, "PAU", "beta", "/work/beta", "/tasks/beta", true).await;

    let projects = list_projects::execute(ProjectStatusFilter::ActiveOnly, &pool)
        .await
        .unwrap();

    assert_eq!(
        projects
            .iter()
            .map(|project| project.title.as_ref())
            .collect::<Vec<_>>(),
        ["alpha", "zeta"]
    );
    assert!(projects.iter().all(|project| !project.is_paused));
    pool.close().await;
}
