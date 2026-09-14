use pwf_models::project::ProjectId;
use pwf_wire::project::ProjectStateChange;

use crate::ports::project_store::ProjectStore;

#[derive(Debug, thiserror::Error)]
pub enum PauseProjectError {
    #[error("project not found: {id}")]
    ProjectNotFound { id: ProjectId },
    #[error("{context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: anyhow::Error,
    },
}

#[cqrsy::command]
pub async fn execute(
    project_id: ProjectId,
    projects: &impl ProjectStore,
) -> Result<ProjectStateChange, PauseProjectError> {
    projects.pause_project(project_id).await
}
