use pwf_models::{
    project::{Project, ProjectId},
    task::TaskId,
};

use crate::project::{get_active_project, get_project::GetProjectError};

#[derive(Debug, thiserror::Error)]
pub enum ResolveTaskProjectError {
    #[error("Unknown project ID `{project_id}` for task {task_id}")]
    UnknownProjectId {
        task_id: TaskId,
        project_id: ProjectId,
    },
    #[error(transparent)]
    QueryProject(anyhow::Error),
}

#[cqrsy::query]
pub async fn execute(
    id: TaskId,
    pool: &sqlx::SqlitePool,
) -> Result<Project, ResolveTaskProjectError> {
    let project = get_active_project::execute(id.project_id().clone(), pool)
        .await
        .map_err(|error| match error {
            GetProjectError::ProjectNotFound { id: project_id } => {
                ResolveTaskProjectError::UnknownProjectId {
                    task_id: id.clone(),
                    project_id,
                }
            }
            error @ GetProjectError::Unexpected { .. } => {
                ResolveTaskProjectError::QueryProject(anyhow::Error::new(error))
            }
        })?;

    Ok(project)
}
