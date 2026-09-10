use pwf_application::project::get_projects;

use crate::support::{insert_project, insert_unrelated_invalid_project};

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn referenced_projects_include_paused_and_skip_missing_ids(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    insert_project(&pool, "AUX", "aux", "/work/aux", "/tasks/aux", true).await;
    insert_unrelated_invalid_project(&pool).await;
    let ids = ["FOO", "AUX", "MISS", "FOO"]
        .map(|id| id.parse().unwrap())
        .into();
    let projects = get_projects::execute(&ids, &pool).await.unwrap();
    assert_eq!(
        projects
            .iter()
            .map(|project| project.id.as_ref())
            .collect::<Vec<_>>(),
        ["AUX", "FOO"]
    );
    assert!(projects[0].is_paused);
    assert!(
        get_projects::execute(&std::collections::BTreeSet::new(), &pool)
            .await
            .unwrap()
            .is_empty()
    );
}
