use pwf_application::project::{add_project, add_project::*};
use pwf_models::project::{
    HomeDirectory, ProjectId, ProjectName, ProjectSource, ProjectSourceKind, ProjectSourceValue,
    ProjectTasks, ProjectTasksKind, ProjectTasksPath,
};
use pwf_wire::project::ProjectFields;

use crate::support::insert_project;

fn project(project_id: &str, title: &str, source_value: &str, tasks_path: &str) -> ProjectFields {
    ProjectFields {
        obsidian_vault: None,
        id: project_id.parse().unwrap(),
        title: ProjectName::try_new(title).unwrap(),
        source: Some(ProjectSource::new(
            ProjectSourceKind::Directory,
            ProjectSourceValue::try_new(source_value).unwrap(),
        )),
        tasks: ProjectTasks::new(
            ProjectTasksKind::Directory,
            ProjectTasksPath::try_new(tasks_path).unwrap(),
        ),
    }
}

fn home() -> HomeDirectory {
    HomeDirectory::new("/home/tester".into())
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn matching_source_row_is_reused(pool: sqlx::SqlitePool) {
    let home = home();

    add_project::execute(
        project("ONE", "one", "/work/shared", "/tasks/one"),
        &pool,
        &home,
    )
    .await
    .unwrap();
    add_project::execute(
        project("TWO", "two", "/work/shared", "/tasks/two"),
        &pool,
        &home,
    )
    .await
    .unwrap();

    let source_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_sources")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(source_count, 1);
    pool.close().await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn duplicate_project_id_is_classified(pool: sqlx::SqlitePool) {
    let home = home();
    add_project::execute(
        project("FOO", "foo", "/work/foo", "/tasks/foo"),
        &pool,
        &home,
    )
    .await
    .unwrap();

    let error = add_project::execute(
        project("foo", "other", "/work/other", "/tasks/other"),
        &pool,
        &home,
    )
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        AddProjectError::DuplicateProjectId { id }
            if id == ProjectId::try_new("FOO").unwrap()
    ));
    pool.close().await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn duplicate_project_title_is_classified(pool: sqlx::SqlitePool) {
    let home = home();
    add_project::execute(
        project("FOO", "foo", "/work/foo", "/tasks/foo"),
        &pool,
        &home,
    )
    .await
    .unwrap();

    let error = add_project::execute(
        project("ALT", "foo", "/work/other", "/tasks/other"),
        &pool,
        &home,
    )
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        AddProjectError::DuplicateProjectTitle { title }
            if title == ProjectName::try_new("foo").unwrap()
    ));
    pool.close().await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn failed_project_insert_rolls_back_new_source(pool: sqlx::SqlitePool) {
    let home = home();
    add_project::execute(
        project("FOO", "foo", "/work/foo", "/tasks/foo"),
        &pool,
        &home,
    )
    .await
    .unwrap();

    let error = add_project::execute(
        project("ALT", "foo", "/work/rolled-back", "/tasks/other"),
        &pool,
        &home,
    )
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        AddProjectError::DuplicateProjectTitle { .. }
    ));
    let source_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_sources WHERE value = ?")
            .bind("/work/rolled-back")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(source_count, 0);
    pool.close().await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn runtime_alias_of_paused_project_is_rejected(pool: sqlx::SqlitePool) {
    let home_path = std::path::PathBuf::from("/home/tester");
    let home = HomeDirectory::new(home_path.clone());
    let resolved_path = home_path.join("tasks/shared");
    insert_project(&pool, "FOO", "foo", "/work/foo", "~/tasks/shared", true).await;

    let error = add_project::execute(
        project(
            "ALT",
            "other",
            "/work/ALT",
            &resolved_path.to_string_lossy(),
        ),
        &pool,
        &home,
    )
    .await
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        format!(
            "managed projects ALT and FOO resolve to the same task location: {}",
            resolved_path.display()
        )
    );
    pool.close().await;
}
