use std::path::PathBuf;

use pwf_models::project::{HomeDirectory, ProjectId, ProjectIdentity, ProjectName, ProjectTasks};
use pwf_wire::project::{GetProject, ProjectFields, ProjectStatusFilter, RenameProject};

use super::{
    Project, TaskLocationError,
    get_project::{self, GetProjectError},
    runtime_path,
};
use crate::ports::{
    project_store::ProjectStore,
    project_task_files::{
        ProjectTaskFilesClient, ProjectTaskFilesRenameCommit, StagedProjectTaskFilesRename,
    },
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
    project_store: &impl ProjectStore,
    task_files: &impl ProjectTaskFilesClient,
    home: &HomeDirectory,
) -> Result<Project, RenameProjectError> {
    let current = get_project::execute(
        GetProject {
            id: command.current_id.clone(),
            status: ProjectStatusFilter::IncludingPaused,
        },
        project_store,
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
    let renamed = match project_store.rename_project(command, &current, home).await {
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
            let rollback = project_store
                .rename_project(
                    RenameProject {
                        current_id: renamed.id.clone(),
                        fields: project_fields(&current),
                    },
                    &renamed,
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
