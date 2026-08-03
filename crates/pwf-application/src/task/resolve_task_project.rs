use pwf_models::{project::Project, task::TaskId};

use super::identifier;
use crate::project::{
    get_active_project::{self, GetActiveProject},
    get_project::GetProjectError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveTaskProject {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveTaskProjectOk {
    pub id: TaskId,
    pub project: Project,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveTaskProjectError {
    #[error("Task not found: {id}")]
    TaskNotFound { id: String },
    #[error("Unknown task id prefix `{prefix}` for {task_identifier}")]
    UnknownPrefix {
        task_identifier: String,
        prefix: String,
    },
    #[error("{0}")]
    QueryProject(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[cqrsy::query]
pub async fn execute(
    query: ResolveTaskProject,
    pool: &sqlx::SqlitePool,
) -> Result<ResolveTaskProjectOk, ResolveTaskProjectError> {
    let id = identifier::parse(&query.id)
        .ok_or(ResolveTaskProjectError::TaskNotFound { id: query.id })?;
    let project = get_active_project::execute(
        GetActiveProject {
            id: id.project_id(),
        },
        pool,
    )
    .await
    .map_err(|error| match error {
        GetProjectError::ProjectNotFound { id: prefix } => ResolveTaskProjectError::UnknownPrefix {
            task_identifier: id.to_string(),
            prefix: prefix.to_string(),
        },
        error @ GetProjectError::Unexpected { .. } => {
            ResolveTaskProjectError::QueryProject(Box::new(error))
        }
    })?;

    Ok(ResolveTaskProjectOk { id, project })
}
