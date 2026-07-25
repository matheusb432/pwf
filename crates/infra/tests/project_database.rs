use pwf_application::{
    AppDbStore,
    project::{
        add::{self, AddProject, AddProjectError, AddProjectFields},
        get::{self, GetProject},
        list::{self, ListProjects},
        load_active::{self, LoadActiveProjects},
        pause::{self, PauseProject, PauseProjectError},
        resume::{self, ResumeProject, ResumeProjectError},
    },
};
use pwf_domain::project::{
    ProjectName, ProjectPrefix, ProjectSource, ProjectSourceKind, ProjectSourceValue, ProjectTasks,
    ProjectTasksKind, ProjectTasksPath,
};

async fn database() -> (tempfile::TempDir, pwf_infra::SqliteStore) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("projects.sqlite3");
    let pool = pwf_infra::database::build_pool(&path).await.unwrap();
    pwf_infra::database::migrate_database(&pool).await.unwrap();
    (directory, pwf_infra::SqliteStore::new(pool))
}

fn id(value: &str) -> ProjectPrefix {
    ProjectPrefix::try_new(value).unwrap()
}

fn directory_project(id: &str, title: &str, source: &str, tasks_path: &str) -> AddProject {
    AddProject {
        fields: AddProjectFields {
            id: ProjectPrefix::try_new(id).unwrap(),
            title: ProjectName::try_new(title).unwrap(),
            source: ProjectSource::new(
                ProjectSourceKind::Directory,
                ProjectSourceValue::try_new(source).unwrap(),
            ),
            tasks: ProjectTasks::new(
                ProjectTasksKind::Directory,
                ProjectTasksPath::try_new(tasks_path).unwrap(),
            ),
        },
        home: std::path::PathBuf::from("/home/runtime"),
    }
}

#[tokio::test]
async fn add_returns_the_complete_active_project() {
    let (_directory, database) = database().await;

    let project = add::execute(
        directory_project("PWF", "pwf", " /work/pwf ", " /tasks/pwf "),
        &database,
    )
    .await
    .unwrap();

    assert_eq!(project.id.as_ref(), "PWF");
    assert_eq!(project.title.as_ref(), "pwf");
    assert_eq!(project.source.kind(), ProjectSourceKind::Directory);
    assert_eq!(project.source.value().as_ref(), " /work/pwf ");
    assert_eq!(project.tasks.kind(), ProjectTasksKind::Directory);
    assert_eq!(project.tasks.path().as_ref(), " /tasks/pwf ");
    assert!(!project.created_at.is_empty());
    assert!(!project.is_paused);
}

#[tokio::test]
async fn add_reuses_a_matching_source_row() {
    let (_directory, database) = database().await;
    add::execute(
        directory_project("ONE", "one", "/work/shared", "/tasks/one"),
        &database,
    )
    .await
    .unwrap();

    add::execute(
        directory_project("TWO", "two", "/work/shared", "/tasks/two"),
        &database,
    )
    .await
    .unwrap();

    let source_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_sources")
        .fetch_one(database.pool())
        .await
        .unwrap();
    assert_eq!(source_count, 1);
}

#[tokio::test]
async fn add_reports_a_duplicate_project_id() {
    let (_directory, database) = database().await;
    add::execute(
        directory_project("PWF", "pwf", "/work/pwf", "/tasks/pwf"),
        &database,
    )
    .await
    .unwrap();

    let error = add::execute(
        directory_project("pwf", "other", "/work/other", "/tasks/other"),
        &database,
    )
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        AddProjectError::DuplicateProjectId { id: duplicate_id }
            if duplicate_id == id("PWF")
    ));
}

#[tokio::test]
async fn add_reports_a_duplicate_project_title() {
    let (_directory, database) = database().await;
    add::execute(
        directory_project("PWF", "pwf", "/work/pwf", "/tasks/pwf"),
        &database,
    )
    .await
    .unwrap();

    let error = add::execute(
        directory_project("ALT", "pwf", "/work/other", "/tasks/other"),
        &database,
    )
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        AddProjectError::DuplicateProjectTitle { title }
            if title == ProjectName::try_new("pwf").unwrap()
    ));
}

#[tokio::test]
async fn add_reports_a_duplicate_runtime_task_location() {
    let (_directory, database) = database().await;
    add::execute(
        directory_project("PWF", "pwf", "/work/pwf", "/tasks/shared"),
        &database,
    )
    .await
    .unwrap();

    let error = add::execute(
        directory_project("ALT", "other", "/work/other", "/tasks/shared"),
        &database,
    )
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        AddProjectError::DuplicateRuntimeTaskLocation {
            first_id,
            second_id,
            path,
        } if first_id == id("ALT") && second_id == id("PWF") && path == std::path::Path::new("/tasks/shared")
    ));
}

#[tokio::test]
async fn add_rolls_back_a_source_inserted_by_a_failed_request() {
    let (_directory, database) = database().await;
    add::execute(
        directory_project("PWF", "pwf", "/work/pwf", "/tasks/pwf"),
        &database,
    )
    .await
    .unwrap();

    let error = add::execute(
        directory_project("ALT", "pwf", "/work/rolled-back", "/tasks/other"),
        &database,
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
            .fetch_one(database.pool())
            .await
            .unwrap();
    assert_eq!(source_count, 0);
}

#[tokio::test]
async fn get_normalizes_a_lowercase_project_id() {
    let (_directory, database) = database().await;
    add::execute(
        directory_project("PWF", "pwf", "/work/pwf", "/tasks/pwf"),
        &database,
    )
    .await
    .unwrap();

    let project = get::execute(GetProject { id: id("pwf") }, &database)
        .await
        .unwrap();

    assert_eq!(project.id.as_ref(), "PWF");
    assert_eq!(project.title.as_ref(), "pwf");
}

#[tokio::test]
async fn list_sorts_by_title_and_optionally_includes_paused_projects() {
    let (_directory, database) = database().await;
    add::execute(
        directory_project("ZED", "zeta", "/work/zeta", "/tasks/zeta"),
        &database,
    )
    .await
    .unwrap();
    add::execute(
        directory_project("ALP", "alpha", "/work/alpha", "/tasks/alpha"),
        &database,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE projects SET paused_at = '2026-07-25T00:00:00.000Z' WHERE id = 'ALP'")
        .execute(database.pool())
        .await
        .unwrap();

    let active = list::execute(
        ListProjects {
            include_paused: false,
        },
        &database,
    )
    .await
    .unwrap();
    let all = list::execute(
        ListProjects {
            include_paused: true,
        },
        &database,
    )
    .await
    .unwrap();

    assert_eq!(
        active
            .iter()
            .map(|project| project.title.as_ref())
            .collect::<Vec<_>>(),
        ["zeta"]
    );
    assert_eq!(
        all.iter()
            .map(|project| project.title.as_ref())
            .collect::<Vec<_>>(),
        ["alpha", "zeta"]
    );
    assert!(all[0].is_paused);
    assert!(!all[1].is_paused);
}

#[tokio::test]
async fn load_active_resolves_runtime_paths_without_changing_persisted_values() {
    let (_directory, database) = database().await;
    add::execute(
        directory_project("PWF", "pwf", "~/tools/pwf", "~/tasks/pwf"),
        &database,
    )
    .await
    .unwrap();
    add::execute(
        directory_project("ARC", "repository", "/work/repository", "/tasks/repository"),
        &database,
    )
    .await
    .unwrap();
    pause::execute(PauseProject { id: id("ARC") }, &database)
        .await
        .unwrap();

    let home = std::path::PathBuf::from("/home/runtime");
    let runtime = load_active::execute(LoadActiveProjects { home: home.clone() }, &database)
        .await
        .unwrap();

    assert_eq!(runtime.len(), 1);
    assert_eq!(runtime[0].project.id, id("PWF"));
    assert_eq!(runtime[0].source_path, home.join("tools/pwf"));
    assert_eq!(runtime[0].tasks_path, home.join("tasks/pwf"));

    let persisted = get::execute(GetProject { id: id("PWF") }, &database)
        .await
        .unwrap();
    assert_eq!(persisted.source.value().as_ref(), "~/tools/pwf");
    assert_eq!(persisted.tasks.path().as_ref(), "~/tasks/pwf");
}

#[tokio::test]
async fn pause_changes_once_and_persists_the_paused_state() {
    let (_directory, database) = database().await;
    add::execute(
        directory_project("PWF", "pwf", "/work/pwf", "/tasks/pwf"),
        &database,
    )
    .await
    .unwrap();

    let first = pause::execute(PauseProject { id: id("PWF") }, &database)
        .await
        .unwrap();
    let second = pause::execute(PauseProject { id: id("PWF") }, &database)
        .await
        .unwrap();
    let persisted = get::execute(GetProject { id: id("PWF") }, &database)
        .await
        .unwrap();
    let active = list::execute(
        ListProjects {
            include_paused: false,
        },
        &database,
    )
    .await
    .unwrap();

    assert!(first.changed);
    assert!(first.project.is_paused);
    assert!(!second.changed);
    assert!(second.project.is_paused);
    assert!(persisted.is_paused);
    assert!(active.is_empty());
}

#[tokio::test]
async fn pause_reports_a_missing_project() {
    let (_directory, database) = database().await;

    let error = pause::execute(PauseProject { id: id("PWF") }, &database)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        PauseProjectError::ProjectNotFound { id: missing_id }
            if missing_id == id("PWF")
    ));
}

#[tokio::test]
async fn resume_changes_once_and_persists_the_active_state() {
    let (_directory, database) = database().await;
    add::execute(
        directory_project("PWF", "pwf", "/work/pwf", "/tasks/pwf"),
        &database,
    )
    .await
    .unwrap();
    pause::execute(PauseProject { id: id("PWF") }, &database)
        .await
        .unwrap();

    let first = resume::execute(
        ResumeProject {
            id: id("PWF"),
            home: std::path::PathBuf::from("/home/runtime"),
        },
        &database,
    )
    .await
    .unwrap();
    let second = resume::execute(
        ResumeProject {
            id: id("PWF"),
            home: std::path::PathBuf::from("/home/runtime"),
        },
        &database,
    )
    .await
    .unwrap();
    let persisted = get::execute(GetProject { id: id("PWF") }, &database)
        .await
        .unwrap();
    let active = list::execute(
        ListProjects {
            include_paused: false,
        },
        &database,
    )
    .await
    .unwrap();

    assert!(first.changed);
    assert!(!first.project.is_paused);
    assert!(!second.changed);
    assert!(!second.project.is_paused);
    assert!(!persisted.is_paused);
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, id("PWF"));
}

#[tokio::test]
async fn resume_reports_a_missing_project() {
    let (_directory, database) = database().await;

    let error = resume::execute(
        ResumeProject {
            id: id("PWF"),
            home: std::path::PathBuf::from("/home/runtime"),
        },
        &database,
    )
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        ResumeProjectError::ProjectNotFound { id: missing_id }
            if missing_id == id("PWF")
    ));
}
