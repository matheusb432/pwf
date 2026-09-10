use std::{assert_matches, io, path::Path};

use pwf_application::{
    ports::project_task_files::{
        ProjectTaskFilesClient, ProjectTaskFilesRenameCommit, StagedProjectTaskFilesRename,
    },
    project::{
        TaskLocationError,
        rename_project::{self, RenameProjectError},
    },
};
use pwf_models::project::{
    HomeDirectory, ProjectId, ProjectIdentity, ProjectName, ProjectSource, ProjectSourceKind,
    ProjectSourceValue, ProjectTasks, ProjectTasksKind, ProjectTasksPath,
};
use pwf_wire::project::{ProjectFields, RenameProject};

use crate::support::insert_project;

#[derive(Clone, Copy)]
enum TaskFilesClient {
    Available,
    Missing,
}

struct StagedTaskFiles;

impl ProjectTaskFilesClient for TaskFilesClient {
    type Error = io::Error;
    type StagedRename = StagedTaskFiles;

    fn stage_project_rename(
        &self,
        _source: &Path,
        _destination: &Path,
        _current: &ProjectIdentity,
        _next: &ProjectIdentity,
    ) -> Result<Self::StagedRename, Self::Error> {
        match self {
            Self::Available => Ok(StagedTaskFiles),
            Self::Missing => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "task files are missing",
            )),
        }
    }
}

impl StagedProjectTaskFilesRename for StagedTaskFiles {
    type Error = io::Error;

    fn commit(self) -> Result<ProjectTaskFilesRenameCommit, Self::Error> {
        Ok(ProjectTaskFilesRenameCommit::Complete)
    }

    fn discard(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn fields(project_id: ProjectId, title: &str, source: &str, tasks: &str) -> ProjectFields {
    ProjectFields {
        obsidian_vault: None,
        snapshot_enabled: false,
        id: project_id,
        title: ProjectName::try_new(title).unwrap(),
        source: Some(ProjectSource::new(
            ProjectSourceKind::Directory,
            ProjectSourceValue::try_new(source).unwrap(),
        )),
        tasks: ProjectTasks::new(
            ProjectTasksKind::Directory,
            ProjectTasksPath::try_new(tasks).unwrap(),
        ),
    }
}

fn home() -> HomeDirectory {
    HomeDirectory::new("/home/tester".into())
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn rename_replaces_identity_and_preserves_project_state(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "OLD",
        "sample-app",
        "/self/sample-app",
        "/project-notes/self/sample-app",
        true,
    )
    .await;

    let renamed = rename_project::execute(
        RenameProject {
            current_id: ProjectId::try_new("OLD").unwrap(),
            fields: fields(
                "NEW".parse().unwrap(),
                "renamed-app",
                "/self/renamed-app",
                "/project-notes/self/renamed-app",
            ),
        },
        &pool,
        &TaskFilesClient::Available,
        &home(),
    )
    .await
    .unwrap();

    assert_eq!(renamed.id.as_ref(), "NEW");
    assert_eq!(renamed.title.as_ref(), "renamed-app");
    assert_eq!(
        renamed.source.as_ref().unwrap().value().as_ref(),
        "/self/renamed-app"
    );
    assert_eq!(
        renamed.tasks.path().as_ref(),
        "/project-notes/self/renamed-app"
    );
    assert_eq!(renamed.created_at.as_ref(), "2026-07-26T00:00:00.000Z");
    assert!(renamed.is_paused);
    pool.close().await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn task_file_staging_failure_leaves_registry_unchanged(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "OLD",
        "sample-app",
        "/self/sample-app",
        "/project-notes/self/sample-app",
        false,
    )
    .await;

    let error = rename_project::execute(
        RenameProject {
            current_id: ProjectId::try_new("OLD").unwrap(),
            fields: fields(
                "NEW".parse().unwrap(),
                "renamed-app",
                "/self/renamed-app",
                "/project-notes/self/renamed-app",
            ),
        },
        &pool,
        &TaskFilesClient::Missing,
        &home(),
    )
    .await
    .unwrap_err();

    assert_matches!(error, RenameProjectError::StageTaskFiles { .. });
    let stored: (String, String) =
        sqlx::query_as("SELECT id, title FROM projects WHERE id = 'OLD'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, ("OLD".to_string(), "sample-app".to_string()));
    pool.close().await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn missing_source_project_is_classified(pool: sqlx::SqlitePool) {
    let error = rename_project::execute(
        RenameProject {
            current_id: ProjectId::try_new("OLD").unwrap(),
            fields: fields(
                "NEW".parse().unwrap(),
                "renamed-app",
                "/self/renamed-app",
                "/project-notes/self/renamed-app",
            ),
        },
        &pool,
        &TaskFilesClient::Available,
        &home(),
    )
    .await
    .unwrap_err();

    assert_matches!(
        error,
        RenameProjectError::SourceProjectNotFound { id }
            if id == ProjectId::try_new("OLD").unwrap()
    );
    pool.close().await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn destination_id_conflict_leaves_source_unchanged(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "OLD",
        "sample-app",
        "/self/sample-app",
        "/project-notes/self/sample-app",
        false,
    )
    .await;
    insert_project(
        &pool,
        "NEW",
        "other",
        "/self/other",
        "/project-notes/self/other",
        false,
    )
    .await;

    let error = rename_project::execute(
        RenameProject {
            current_id: ProjectId::try_new("OLD").unwrap(),
            fields: fields(
                "NEW".parse().unwrap(),
                "renamed-app",
                "/self/renamed-app",
                "/project-notes/self/renamed-app",
            ),
        },
        &pool,
        &TaskFilesClient::Available,
        &home(),
    )
    .await
    .unwrap_err();

    assert_matches!(
        error,
        RenameProjectError::DestinationProjectIdExists { id }
            if id == ProjectId::try_new("NEW").unwrap()
    );
    let source: (String, String, String) =
        sqlx::query_as("SELECT id, title, tasks_path FROM projects WHERE id = 'OLD'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        source,
        (
            "OLD".to_string(),
            "sample-app".to_string(),
            "/project-notes/self/sample-app".to_string(),
        )
    );
    pool.close().await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn destination_title_conflict_is_classified(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "OLD",
        "sample-app",
        "/self/sample-app",
        "/project-notes/self/sample-app",
        false,
    )
    .await;
    insert_project(
        &pool,
        "ALT",
        "renamed-app",
        "/self/other",
        "/project-notes/self/other",
        false,
    )
    .await;

    let error = rename_project::execute(
        RenameProject {
            current_id: ProjectId::try_new("OLD").unwrap(),
            fields: fields(
                "NEW".parse().unwrap(),
                "renamed-app",
                "/self/renamed-app",
                "/project-notes/self/renamed-app",
            ),
        },
        &pool,
        &TaskFilesClient::Available,
        &home(),
    )
    .await
    .unwrap_err();

    assert_matches!(
        error,
        RenameProjectError::DestinationProjectTitleExists { title }
            if title == ProjectName::try_new("renamed-app").unwrap()
    );
    pool.close().await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn runtime_task_collision_leaves_source_unchanged(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "OLD",
        "sample-app",
        "/self/sample-app",
        "/project-notes/self/sample-app",
        false,
    )
    .await;
    insert_project(
        &pool,
        "ALT",
        "other",
        "/self/other",
        "~/tasks/shared",
        false,
    )
    .await;

    let error = rename_project::execute(
        RenameProject {
            current_id: ProjectId::try_new("OLD").unwrap(),
            fields: fields(
                "NEW".parse().unwrap(),
                "renamed-app",
                "/self/renamed-app",
                "/home/tester/tasks/shared",
            ),
        },
        &pool,
        &TaskFilesClient::Available,
        &home(),
    )
    .await
    .unwrap_err();

    assert_matches!(
        error,
        RenameProjectError::TaskLocation(TaskLocationError::Collision {
            first_id,
            second_id,
            ..
        }) if first_id == ProjectId::try_new("ALT").unwrap()
            && second_id == ProjectId::try_new("NEW").unwrap()
    );
    let title: String = sqlx::query_scalar("SELECT title FROM projects WHERE id = 'OLD'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(title, "sample-app");
    pool.close().await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn invalid_task_path_is_classified(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "OLD",
        "sample-app",
        "/self/sample-app",
        "/project-notes/self/sample-app",
        false,
    )
    .await;

    let error = rename_project::execute(
        RenameProject {
            current_id: ProjectId::try_new("OLD").unwrap(),
            fields: fields(
                "NEW".parse().unwrap(),
                "renamed-app",
                "/self/renamed-app",
                "~/tasks/../renamed-app",
            ),
        },
        &pool,
        &TaskFilesClient::Available,
        &home(),
    )
    .await
    .unwrap_err();

    assert_matches!(
        error,
        RenameProjectError::TaskLocation(TaskLocationError::InvalidPath { .. })
    );
    pool.close().await;
}
