use pwf_models::project::ProjectId;
use pwf_wire::project::UpdateProject;

use crate::ports::project_store::ProjectStore;

#[derive(Debug, thiserror::Error)]
pub enum UpdateProjectError {
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
    command: UpdateProject,
    projects: &impl ProjectStore,
) -> Result<(), UpdateProjectError> {
    projects.update_project(command).await
}
