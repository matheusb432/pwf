use std::{error::Error, path::PathBuf};

use pwf_models::project::{
    HomeDirectory, ProjectId, ProjectIdentity, ProjectName, ProjectTasks, ProjectTasksPath,
};
use pwf_wire::project::{GetProject, ProjectFields, ProjectStatusFilter, RenameProject};

use super::{
    Project, TaskLocationError,
    get_project::{self, GetProjectError},
    runtime_path, source_record, task_location,
};
use crate::ports::project_task_files::{
    ProjectTaskFilesClient, ProjectTaskFilesRenameCommit, StagedProjectTaskFilesRename,
};

#[derive(Debug, thiserror::Error)]
pub enum RenameProjectError {
    #[error("project rename failed: project not found: {id}")]
    SourceProjectNotFound { id: ProjectId },
    #[error("project rename failed: project changed while task files were staged: {id}")]
    SourceProjectChanged { id: ProjectId },
    #[error("project rename failed: project id already exists: {id}")]
    DestinationProjectIdExists { id: ProjectId },
    #[error("project rename failed: project title already exists: {title}")]
    DestinationProjectTitleExists { title: ProjectName },
    #[error("project rename failed: {0}")]
    TaskLocation(#[from] TaskLocationError),
    #[error("project rename failed: {context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: anyhow::Error,
    },
    #[error("project rename staging failed: {source}")]
    StageTaskFiles {
        #[source]
        source: anyhow::Error,
    },
    #[error("{rename_error}; removing staging directory failed: {discard_error}")]
    DiscardTaskFiles {
        rename_error: Box<Self>,
        discard_error: anyhow::Error,
    },
    #[error(
        "project rename committed, but removing filesystem backup {} failed: {source}; registry remains renamed",
        path.display()
    )]
    BackupRetained {
        path: PathBuf,
        #[source]
        source: anyhow::Error,
    },
    #[error("project rename filesystem commit failed: {commit_error}; registry rollback succeeded")]
    TaskFilesCommitRolledBack { commit_error: anyhow::Error },
    #[error(
        "project rename filesystem commit failed: {commit_error}; registry rollback failed: {rollback_error}"
    )]
    TaskFilesCommitRollbackFailed {
        commit_error: anyhow::Error,
        rollback_error: Box<Self>,
    },
}

/// Replaces one managed project's registry identity, locations, and task files.
#[cqrsy::command]
pub async fn execute(
    command: RenameProject,
    pool: &sqlx::SqlitePool,
    task_files: &impl ProjectTaskFilesClient,
    home: &HomeDirectory,
) -> Result<Project, RenameProjectError> {
    let current = get_project::execute(
        GetProject {
            id: command.current_id.clone(),
            status: ProjectStatusFilter::IncludingPaused,
        },
        pool,
    )
    .await
    .map_err(get_project_error)?;
    let source_tasks = resolve_tasks_path(&current.id, &current.tasks, home)?;
    let destination_tasks = resolve_tasks_path(&command.fields.id, &command.fields.tasks, home)?;
    let current_identity = ProjectIdentity::new(current.id.clone(), current.title.clone());
    let next_identity =
        ProjectIdentity::new(command.fields.id.clone(), command.fields.title.clone());
    let staged = task_files
        .stage_project_rename(
            &source_tasks,
            &destination_tasks,
            &current_identity,
            &next_identity,
        )
        .map_err(|source| RenameProjectError::StageTaskFiles {
            source: anyhow::Error::new(source),
        })?;
    let renamed = match rename_registry(command, &current, pool, home).await {
        Ok(renamed) => renamed,
        Err(rename_error) => {
            return match staged.discard() {
                Ok(()) => Err(rename_error),
                Err(discard_error) => Err(RenameProjectError::DiscardTaskFiles {
                    rename_error: Box::new(rename_error),
                    discard_error: anyhow::Error::new(discard_error),
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
                },
                &renamed,
                pool,
                home,
            )
            .await;
            match rollback {
                Ok(_) => Err(RenameProjectError::TaskFilesCommitRolledBack {
                    commit_error: anyhow::Error::new(commit_error),
                }),
                Err(rollback_error) => Err(RenameProjectError::TaskFilesCommitRollbackFailed {
                    commit_error: anyhow::Error::new(commit_error),
                    rollback_error: Box::new(rollback_error),
                }),
            }
        }
    }
}

async fn rename_registry(
    command: RenameProject,
    expected_current: &Project,
    pool: &sqlx::SqlitePool,
    home: &HomeDirectory,
) -> Result<Project, RenameProjectError> {
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| unexpected("starting project rename transaction", error))?;
    validate_identity(&mut transaction, &command, expected_current).await?;
    let destination_id = command.fields.id.as_ref();
    let existing = other_task_locations(&mut transaction, &command.current_id).await?;
    task_location::reject_collision(
        &command.fields.id,
        command.fields.tasks.path(),
        existing,
        home,
    )?;
    let source_id = match &command.fields.source {
        Some(source) => Some(
            source_record::get_or_insert(&mut transaction, source)
                .await
                .map_err(|error| unexpected("resolving destination project source", error))?,
        ),
        None => None,
    };
    replace_project_row(&mut transaction, &command, source_id).await?;

    let row = project_query!("WHERE projects.id = ?", destination_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|error| unexpected("reading renamed project", error))?;
    let project = super::project_from_row(row)
        .map_err(|error| unexpected("converting renamed project", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| unexpected("committing project rename", error))?;
    Ok(project)
}

async fn replace_project_row(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    command: &RenameProject,
    source_id: Option<i64>,
) -> Result<(), RenameProjectError> {
    let current_id = command.current_id.as_ref();
    let destination_id = command.fields.id.as_ref();
    let destination_title = command.fields.title.as_ref();
    let tasks_kind = command.fields.tasks.kind().to_string();
    let tasks_path = command.fields.tasks.path().as_ref();
    let obsidian_vault = command.fields.obsidian_vault.as_ref().map(AsRef::as_ref);
    let update = sqlx::query!(
        r#"
        UPDATE projects
        SET
            id = ?,
            project_source_id = ?,
            title = ?,
            tasks_kind = ?,
            tasks_path = ?,
            obsidian_vault = ?
        WHERE id = ?
        "#,
        destination_id,
        source_id,
        destination_title,
        tasks_kind,
        tasks_path,
        obsidian_vault,
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
    project_id: &ProjectId,
    tasks: &ProjectTasks,
    home: &HomeDirectory,
) -> Result<PathBuf, RenameProjectError> {
    runtime_path::resolve(tasks.path().as_ref(), home)
        .map(|resolved| resolved.path().to_path_buf())
        .map_err(|source| {
            TaskLocationError::InvalidPath {
                project_id: project_id.clone(),
                path: tasks.path().clone(),
                source,
            }
            .into()
        })
}

fn project_fields(project: &Project) -> ProjectFields {
    ProjectFields {
        id: project.id.clone(),
        title: project.title.clone(),
        source: project.source.clone(),
        tasks: project.tasks.clone(),
        obsidian_vault: project.obsidian_vault.clone(),
        snapshot_enabled: project.snapshot_enabled,
    }
}

async fn validate_identity(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    command: &RenameProject,
    expected_current: &Project,
) -> Result<(), RenameProjectError> {
    let current_id = command.current_id.as_ref();
    let current_row = project_query!("WHERE projects.id = ?", current_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|error| unexpected("reading source project", error))?;
    let Some(current_row) = current_row else {
        return Err(RenameProjectError::SourceProjectNotFound {
            id: command.current_id.clone(),
        });
    };
    let current = super::project_from_row(current_row)
        .map_err(|error| unexpected("converting source project", error))?;
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
    current_id: &ProjectId,
) -> Result<Vec<(ProjectId, ProjectTasksPath)>, RenameProjectError> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT id, tasks_path FROM projects WHERE id != ? ORDER BY id ASC",
    )
    .bind(current_id.as_ref())
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| unexpected("reading project task locations", error))?
    .into_iter()
    .map(|(id, tasks_path)| {
        let id = ProjectId::try_new(id)
            .map_err(|error| unexpected("converting project task location id", error))?;
        let tasks_path = ProjectTasksPath::try_new(tasks_path)
            .map_err(|error| unexpected("converting project task location path", error))?;
        Ok((id, tasks_path))
    })
    .collect()
}

fn unexpected(
    context: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> RenameProjectError {
    RenameProjectError::Unexpected {
        context,
        source: anyhow::Error::new(source),
    }
}
