use pwf_models::{
    project::{Project, ProjectId},
    task::TaskId,
};

use crate::project::{
    get_active_project::{self, GetActiveProject},
    get_project::GetProjectError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveTaskProject {
    pub id: TaskId,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveTaskProjectError {
    #[error("Unknown project ID `{project_id}` for task {task_id}")]
    UnknownProjectId {
        task_id: TaskId,
        project_id: ProjectId,
    },
    #[error("{0}")]
    QueryProject(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[cqrsy::query]
pub async fn execute(
    query: ResolveTaskProject,
    pool: &sqlx::SqlitePool,
) -> Result<Project, ResolveTaskProjectError> {
    let id = query.id;
    let project = get_active_project::execute(
        GetActiveProject {
            id: id.project_id().clone(),
        },
        pool,
    )
    .await
    .map_err(|error| match error {
        GetProjectError::ProjectNotFound { id: project_id } => {
            ResolveTaskProjectError::UnknownProjectId {
                task_id: id.clone(),
                project_id,
            }
        }
        error @ GetProjectError::Unexpected { .. } => {
            ResolveTaskProjectError::QueryProject(Box::new(error))
        }
    })?;

    Ok(project)
}
