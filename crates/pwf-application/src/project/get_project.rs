use pwf_models::project::{Project, ProjectId};
use pwf_wire::project::GetProject;

use crate::ports::project_store::ProjectStore;

#[derive(Debug, thiserror::Error)]
pub enum GetProjectError {
    #[error("project not found: {id}")]
    ProjectNotFound { id: ProjectId },
    #[error("{context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: anyhow::Error,
    },
}

#[cqrsy::query]
pub async fn execute(
    query: GetProject,
    projects: &impl ProjectStore,
) -> Result<Project, GetProjectError> {
    projects.get_project(query).await
}
