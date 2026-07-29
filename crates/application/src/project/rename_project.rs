use std::{
    error::Error,
    path::{Path, PathBuf},
};

use pwf_models::project::{
    ProjectIndexIdentity, ProjectName, ProjectPrefix, ProjectSource, ProjectTasks,
};

use super::{
    Project,
    add_project::AddProjectFields,
    dto::{ProjectRow, ProjectRowError},
    get_project::{self, GetProject, GetProjectError},
    resolve_runtime_path::{self, ResolveRuntimePath},
    task_location::{self, TaskLocationError},
};
use crate::{
    AppDbStore, ProjectTaskFilesClient, ProjectTaskFilesRenameCommit, StagedProjectTaskFilesRename,
};

/// Requests replacement of one managed project's identity and locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameProject {
    /// Existing project prefix.
    pub current_id: ProjectPrefix,
    /// Replacement project fields.
    pub fields: AddProjectFields,
    /// Home directory used to expand home-relative task paths.
    pub home: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum RenameProjectError {
    #[error("project rename failed: project not found: {id}")]
    SourceProjectNotFound { id: ProjectPrefix },
    #[error("project rename failed: project changed while task files were staged: {id}")]
    SourceProjectChanged { id: ProjectPrefix },
    #[error("project rename failed: project id already exists: {id}")]
    DestinationProjectIdExists { id: ProjectPrefix },
    #[error("project rename failed: project title already exists: {title}")]
    DestinationProjectTitleExists { title: ProjectName },
    #[error(
        "project rename failed: managed project {project_id} task path '{path}' is invalid: {source}"
    )]
    InvalidTaskPath {
        project_id: ProjectPrefix,
        path: String,
        #[source]
        source: super::resolve_runtime_path::RuntimePathError,
    },
    #[error(
        "project rename failed: managed projects {first_id} and {second_id} resolve to the same task location: {}",
        path.display()
    )]
    DuplicateRuntimeTaskLocation {
        first_id: ProjectPrefix,
        second_id: ProjectPrefix,
        path: PathBuf,
    },
    #[error("project rename failed: {context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
    #[error("project rename staging failed: {source}")]
    StageTaskFiles {
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
    #[error("{rename_error}; removing staging directory failed: {discard_error}")]
    DiscardTaskFiles {
        rename_error: Box<Self>,
        discard_error: Box<dyn Error + Send + Sync>,
    },
    #[error(
        "project rename committed, but removing filesystem backup {} failed: {source}; registry remains renamed",
        path.display()
    )]
    BackupRetained {
        path: PathBuf,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
    #[error("project rename filesystem commit failed: {commit_error}; registry rollback succeeded")]
    TaskFilesCommitRolledBack {
        commit_error: Box<dyn Error + Send + Sync>,
    },
    #[error(
        "project rename filesystem commit failed: {commit_error}; registry rollback failed: {rollback_error}"
    )]
    TaskFilesCommitRollbackFailed {
        commit_error: Box<dyn Error + Send + Sync>,
        rollback_error: Box<Self>,
    },
}

/// Replaces one managed project's registry identity, locations, and task files.
#[cqrsy::command]
pub async fn execute(
    command: RenameProject,
    database: &impl AppDbStore,
    task_files: &impl ProjectTaskFilesClient,
) -> Result<Project, RenameProjectError> {
    let current = get_project::execute(
        GetProject {
            id: command.current_id.clone(),
        },
        database,
    )
    .await
    .map_err(get_project_error)?;
    let source_tasks = resolve_tasks_path(&current.id, &current.tasks, &command.home)?;
    let destination_tasks =
        resolve_tasks_path(&command.fields.id, &command.fields.tasks, &command.home)?;
    let current_identity = ProjectIndexIdentity::new(current.id.clone(), current.title.clone());
    let next_identity =
        ProjectIndexIdentity::new(command.fields.id.clone(), command.fields.title.clone());
    let home = command.home.clone();
    let staged = task_files
        .stage_project_rename(
            &source_tasks,
            &destination_tasks,
            &current_identity,
            &next_identity,
        )
        .map_err(|source| RenameProjectError::StageTaskFiles {
            source: Box::new(source),
        })?;
    let renamed = match rename_registry(command, &current, database).await {
        Ok(renamed) => renamed,
        Err(rename_error) => {
            return match staged.discard() {
                Ok(()) => Err(rename_error),
                Err(discard_error) => Err(RenameProjectError::DiscardTaskFiles {
                    rename_error: Box::new(rename_error),
                    discard_error: Box::new(discard_error),
                }),
            };
        }
    };

    match staged.commit() {
        Ok(ProjectTaskFilesRenameCommit::Complete) => Ok(renamed),
        Ok(ProjectTaskFilesRenameCommit::BackupRetained { path, source }) => {
            Err(RenameProjectError::BackupRetained { path, source })
        }
        Err(commit_error) => {
            let rollback = rename_registry(
                RenameProject {
                    current_id: renamed.id.clone(),
                    fields: project_fields(&current),
                    home,
                },
                &renamed,
                database,
            )
            .await;
            match rollback {
                Ok(_) => Err(RenameProjectError::TaskFilesCommitRolledBack {
                    commit_error: Box::new(commit_error),
                }),
                Err(rollback_error) => Err(RenameProjectError::TaskFilesCommitRollbackFailed {
                    commit_error: Box::new(commit_error),
                    rollback_error: Box::new(rollback_error),
                }),
            }
        }
    }
}

async fn rename_registry(
    command: RenameProject,
    expected_current: &Project,
    database: &impl AppDbStore,
) -> Result<Project, RenameProjectError> {
    let mut transaction = database
        .pool()
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| unexpected("starting project rename transaction", error))?;
    validate_identity(&mut transaction, &command, expected_current).await?;
    let destination_id = command.fields.id.as_ref();
    let existing = other_task_locations(&mut transaction, &command.current_id).await?;
    task_location::reject_collision(
        &command.fields.id,
        command.fields.tasks.path().as_ref(),
        existing,
        &command.home,
    )
    .map_err(task_location_error)?;
    let source_id = destination_source_id(&mut transaction, &command.fields.source).await?;
    replace_project_row(&mut transaction, &command, source_id).await?;

    let row = sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT
            projects.id AS "id!",
            projects.title AS "title!",
            project_sources.kind AS "source_kind!",
            project_sources.value AS "source_value!",
            projects.tasks_kind AS "tasks_kind!",
            projects.tasks_path AS "tasks_path!",
            projects.created_at AS "created_at!",
            (projects.paused_at IS NOT NULL) AS "is_paused!: bool"
        FROM projects
        JOIN project_sources ON project_sources.id = projects.project_source_id
        WHERE projects.id = ?
        "#,
        destination_id,
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| unexpected("reading renamed project", error))?;
    let project = Project::try_from(row)
        .map_err(|error| unexpected_row("converting renamed project", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| unexpected("committing project rename", error))?;
    Ok(project)
}

async fn destination_source_id(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    source: &ProjectSource,
) -> Result<i64, RenameProjectError> {
    let source_kind = source.kind().to_string();
    let source_value = source.value().as_ref();
    let source_id = sqlx::query_scalar!(
        r#"
        SELECT id AS "id!"
        FROM project_sources
        WHERE kind = ? AND value = ?
        "#,
        source_kind,
        source_value,
    )
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| unexpected("reading destination project source", error))?;
    match source_id {
        Some(source_id) => Ok(source_id),
        None => sqlx::query!(
            r#"
            INSERT INTO project_sources (kind, value)
            VALUES (?, ?)
            "#,
            source_kind,
            source_value,
        )
        .execute(&mut **transaction)
        .await
        .map(|result| result.last_insert_rowid())
        .map_err(|error| unexpected("inserting destination project source", error)),
    }
}

async fn replace_project_row(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    command: &RenameProject,
    source_id: i64,
) -> Result<(), RenameProjectError> {
    let current_id = command.current_id.as_ref();
    let destination_id = command.fields.id.as_ref();
    let destination_title = command.fields.title.as_ref();
    let tasks_kind = command.fields.tasks.kind().to_string();
    let tasks_path = command.fields.tasks.path().as_ref();
    let update = sqlx::query!(
        r#"
        UPDATE projects
        SET
            id = ?,
            project_source_id = ?,
            title = ?,
            tasks_kind = ?,
            tasks_path = ?
        WHERE id = ?
        "#,
        destination_id,
        source_id,
        destination_title,
        tasks_kind,
        tasks_path,
        current_id,
    )
    .execute(&mut **transaction)
    .await
    .map_err(|error| unexpected("updating project", error))?;
    match update.rows_affected() {
        1 => Ok(()),
        0 => Err(RenameProjectError::SourceProjectNotFound {
            id: command.current_id.clone(),
        }),
        count => Err(unexpected(
            "updating project",
            std::io::Error::other(format!(
                "project rename updated {count} rows; expected exactly one"
            )),
        )),
    }
}

fn get_project_error(error: GetProjectError) -> RenameProjectError {
    match error {
        GetProjectError::ProjectNotFound { id } => RenameProjectError::SourceProjectNotFound { id },
        GetProjectError::Unexpected { context, source } => {
            RenameProjectError::Unexpected { context, source }
        }
    }
}

fn resolve_tasks_path(
    project_id: &ProjectPrefix,
    tasks: &ProjectTasks,
    home: &Path,
) -> Result<PathBuf, RenameProjectError> {
    resolve_runtime_path::execute(&ResolveRuntimePath {
        path: tasks.path().as_ref().to_string(),
        home: home.to_path_buf(),
    })
    .map(|resolved| resolved.path().to_path_buf())
    .map_err(|source| RenameProjectError::InvalidTaskPath {
        project_id: project_id.clone(),
        path: tasks.path().as_ref().to_string(),
        source,
    })
}

fn project_fields(project: &Project) -> AddProjectFields {
    AddProjectFields {
        id: project.id.clone(),
        title: project.title.clone(),
        source: project.source.clone(),
        tasks: project.tasks.clone(),
    }
}

async fn validate_identity(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    command: &RenameProject,
    expected_current: &Project,
) -> Result<(), RenameProjectError> {
    let current_id = command.current_id.as_ref();
    let current_row = sqlx::query_as!(
        ProjectRow,
        r#"
        SELECT
            projects.id AS "id!",
            projects.title AS "title!",
            project_sources.kind AS "source_kind!",
            project_sources.value AS "source_value!",
            projects.tasks_kind AS "tasks_kind!",
            projects.tasks_path AS "tasks_path!",
            projects.created_at AS "created_at!",
            (projects.paused_at IS NOT NULL) AS "is_paused!: bool"
        FROM projects
        JOIN project_sources ON project_sources.id = projects.project_source_id
        WHERE projects.id = ?
        "#,
        current_id,
    )
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|error| unexpected("reading source project", error))?;
    let Some(current_row) = current_row else {
        return Err(RenameProjectError::SourceProjectNotFound {
            id: command.current_id.clone(),
        });
    };
    let current = Project::try_from(current_row)
        .map_err(|error| unexpected_row("converting source project", error))?;
    if current != *expected_current {
        return Err(RenameProjectError::SourceProjectChanged {
            id: command.current_id.clone(),
        });
    }

    let destination_id = command.fields.id.as_ref();
    let destination_title = command.fields.title.as_ref();
    let conflicts = sqlx::query!(
        r#"
        SELECT
            EXISTS(
                SELECT 1
                FROM projects
                WHERE id = ? AND id != ?
            ) AS "id_exists!: bool",
            EXISTS(
                SELECT 1
                FROM projects
                WHERE title = ? AND id != ?
            ) AS "title_exists!: bool"
        "#,
        destination_id,
        current_id,
        destination_title,
        current_id,
    )
    .fetch_one(&mut **transaction)
    .await
    .map_err(|error| unexpected("classifying project rename conflicts", error))?;
    if conflicts.id_exists {
        return Err(RenameProjectError::DestinationProjectIdExists {
            id: command.fields.id.clone(),
        });
    }
    if conflicts.title_exists {
        return Err(RenameProjectError::DestinationProjectTitleExists {
            title: command.fields.title.clone(),
        });
    }
    Ok(())
}

async fn other_task_locations(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    current_id: &ProjectPrefix,
) -> Result<Vec<(ProjectPrefix, String)>, RenameProjectError> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT id, tasks_path FROM projects WHERE id != ? ORDER BY id ASC",
    )
    .bind(current_id.as_ref())
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| unexpected("reading project task locations", error))?
    .into_iter()
    .map(|(id, tasks_path)| {
        ProjectPrefix::try_new(id)
            .map(|id| (id, tasks_path))
            .map_err(|error| unexpected("converting project task location", error))
    })
    .collect()
}

fn task_location_error(error: TaskLocationError) -> RenameProjectError {
    match error {
        TaskLocationError::InvalidPath {
            project_id,
            path,
            source,
        } => RenameProjectError::InvalidTaskPath {
            project_id,
            path,
            source,
        },
        TaskLocationError::Collision {
            first_id,
            second_id,
            path,
        } => RenameProjectError::DuplicateRuntimeTaskLocation {
            first_id,
            second_id,
            path,
        },
    }
}

fn unexpected(
    context: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> RenameProjectError {
    RenameProjectError::Unexpected {
        context,
        source: Box::new(source),
    }
}

fn unexpected_row(context: &'static str, source: ProjectRowError) -> RenameProjectError {
    unexpected(context, source)
}

#[cfg(test)]
mod tests {
    use std::{
        assert_matches, io,
        path::{Path, PathBuf},
    };

    use pwf_models::project::{
        ProjectIndexIdentity, ProjectName, ProjectPrefix, ProjectSource, ProjectSourceKind,
        ProjectSourceValue, ProjectTasks, ProjectTasksKind, ProjectTasksPath,
    };

    use crate::{
        AppDbStore, ProjectTaskFilesClient, ProjectTaskFilesRenameCommit,
        StagedProjectTaskFilesRename,
        ports::TestDatabase,
        project::{
            add_project::AddProjectFields,
            rename_project::{self, RenameProject, RenameProjectError},
        },
    };

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
            _current: &ProjectIndexIdentity,
            _next: &ProjectIndexIdentity,
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

    fn fields(id: &str, title: &str, source: &str, tasks: &str) -> AddProjectFields {
        AddProjectFields {
            id: ProjectPrefix::try_new(id).unwrap(),
            title: ProjectName::try_new(title).unwrap(),
            source: ProjectSource::new(
                ProjectSourceKind::Directory,
                ProjectSourceValue::try_new(source).unwrap(),
            ),
            tasks: ProjectTasks::new(
                ProjectTasksKind::Directory,
                ProjectTasksPath::try_new(tasks).unwrap(),
            ),
        }
    }

    #[tokio::test]
    async fn rename_replaces_identity_and_preserves_project_state() {
        let database = TestDatabase::new().await;
        database
            .insert_project(
                "SSH",
                "ssh-agent-phone-app",
                "/self/ssh-agent-phone-app",
                "/pwf-db/self/ssh-agent-phone-app",
                true,
            )
            .await;

        let renamed = rename_project::execute(
            RenameProject {
                current_id: ProjectPrefix::try_new("SSH").unwrap(),
                fields: fields("MUX", "mimux", "/self/mimux", "/pwf-db/self/mimux"),
                home: PathBuf::from("/home/tester"),
            },
            &database,
            &TaskFilesClient::Available,
        )
        .await
        .unwrap();

        assert_eq!(renamed.id.as_ref(), "MUX");
        assert_eq!(renamed.title.as_ref(), "mimux");
        assert_eq!(renamed.source.value().as_ref(), "/self/mimux");
        assert_eq!(renamed.tasks.path().as_ref(), "/pwf-db/self/mimux");
        assert_eq!(renamed.created_at, "2026-07-26T00:00:00.000Z");
        assert!(renamed.is_paused);
    }

    #[tokio::test]
    async fn task_file_staging_failure_leaves_registry_unchanged() {
        let database = TestDatabase::new().await;
        database
            .insert_project(
                "SSH",
                "ssh-agent-phone-app",
                "/self/ssh-agent-phone-app",
                "/pwf-db/self/ssh-agent-phone-app",
                false,
            )
            .await;

        let error = rename_project::execute(
            RenameProject {
                current_id: ProjectPrefix::try_new("SSH").unwrap(),
                fields: fields("MUX", "mimux", "/self/mimux", "/pwf-db/self/mimux"),
                home: PathBuf::from("/home/tester"),
            },
            &database,
            &TaskFilesClient::Missing,
        )
        .await
        .unwrap_err();

        assert_matches!(error, RenameProjectError::StageTaskFiles { .. });
        let stored: (String, String) =
            sqlx::query_as("SELECT id, title FROM projects WHERE id = 'SSH'")
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(
            stored,
            ("SSH".to_string(), "ssh-agent-phone-app".to_string())
        );
    }

    #[tokio::test]
    async fn missing_source_project_is_classified() {
        let database = TestDatabase::new().await;

        let error = rename_project::execute(
            RenameProject {
                current_id: ProjectPrefix::try_new("SSH").unwrap(),
                fields: fields("MUX", "mimux", "/self/mimux", "/pwf-db/self/mimux"),
                home: PathBuf::from("/home/tester"),
            },
            &database,
            &TaskFilesClient::Available,
        )
        .await
        .unwrap_err();

        assert_matches!(
            error,
            RenameProjectError::SourceProjectNotFound { id }
                if id == ProjectPrefix::try_new("SSH").unwrap()
        );
    }

    #[tokio::test]
    async fn destination_id_conflict_leaves_source_unchanged() {
        let database = TestDatabase::new().await;
        database
            .insert_project(
                "SSH",
                "ssh-agent-phone-app",
                "/self/ssh-agent-phone-app",
                "/pwf-db/self/ssh-agent-phone-app",
                false,
            )
            .await;
        database
            .insert_project("MUX", "other", "/self/other", "/pwf-db/self/other", false)
            .await;

        let error = rename_project::execute(
            RenameProject {
                current_id: ProjectPrefix::try_new("SSH").unwrap(),
                fields: fields("MUX", "mimux", "/self/mimux", "/pwf-db/self/mimux"),
                home: PathBuf::from("/home/tester"),
            },
            &database,
            &TaskFilesClient::Available,
        )
        .await
        .unwrap_err();

        assert_matches!(
            error,
            RenameProjectError::DestinationProjectIdExists { id }
                if id == ProjectPrefix::try_new("MUX").unwrap()
        );
        let source: (String, String, String) =
            sqlx::query_as("SELECT id, title, tasks_path FROM projects WHERE id = 'SSH'")
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(
            source,
            (
                "SSH".to_string(),
                "ssh-agent-phone-app".to_string(),
                "/pwf-db/self/ssh-agent-phone-app".to_string(),
            )
        );
    }

    #[tokio::test]
    async fn destination_title_conflict_is_classified() {
        let database = TestDatabase::new().await;
        database
            .insert_project(
                "SSH",
                "ssh-agent-phone-app",
                "/self/ssh-agent-phone-app",
                "/pwf-db/self/ssh-agent-phone-app",
                false,
            )
            .await;
        database
            .insert_project("ALT", "mimux", "/self/other", "/pwf-db/self/other", false)
            .await;

        let error = rename_project::execute(
            RenameProject {
                current_id: ProjectPrefix::try_new("SSH").unwrap(),
                fields: fields("MUX", "mimux", "/self/mimux", "/pwf-db/self/mimux"),
                home: PathBuf::from("/home/tester"),
            },
            &database,
            &TaskFilesClient::Available,
        )
        .await
        .unwrap_err();

        assert_matches!(
            error,
            RenameProjectError::DestinationProjectTitleExists { title }
                if title == ProjectName::try_new("mimux").unwrap()
        );
    }

    #[tokio::test]
    async fn runtime_task_collision_leaves_source_unchanged() {
        let database = TestDatabase::new().await;
        database
            .insert_project(
                "SSH",
                "ssh-agent-phone-app",
                "/self/ssh-agent-phone-app",
                "/pwf-db/self/ssh-agent-phone-app",
                false,
            )
            .await;
        database
            .insert_project("ALT", "other", "/self/other", "~/tasks/shared", false)
            .await;

        let error = rename_project::execute(
            RenameProject {
                current_id: ProjectPrefix::try_new("SSH").unwrap(),
                fields: fields("MUX", "mimux", "/self/mimux", "/home/tester/tasks/shared"),
                home: PathBuf::from("/home/tester"),
            },
            &database,
            &TaskFilesClient::Available,
        )
        .await
        .unwrap_err();

        assert_matches!(
            error,
            RenameProjectError::DuplicateRuntimeTaskLocation {
                first_id,
                second_id,
                ..
            } if first_id == ProjectPrefix::try_new("ALT").unwrap()
                && second_id == ProjectPrefix::try_new("MUX").unwrap()
        );
        let title: String = sqlx::query_scalar("SELECT title FROM projects WHERE id = 'SSH'")
            .fetch_one(database.pool())
            .await
            .unwrap();
        assert_eq!(title, "ssh-agent-phone-app");
    }

    #[tokio::test]
    async fn invalid_task_path_is_classified() {
        let database = TestDatabase::new().await;
        database
            .insert_project(
                "SSH",
                "ssh-agent-phone-app",
                "/self/ssh-agent-phone-app",
                "/pwf-db/self/ssh-agent-phone-app",
                false,
            )
            .await;

        let error = rename_project::execute(
            RenameProject {
                current_id: ProjectPrefix::try_new("SSH").unwrap(),
                fields: fields("MUX", "mimux", "/self/mimux", "~/tasks/../mimux"),
                home: PathBuf::from("/home/tester"),
            },
            &database,
            &TaskFilesClient::Available,
        )
        .await
        .unwrap_err();

        assert_matches!(error, RenameProjectError::InvalidTaskPath { .. });
    }
}
