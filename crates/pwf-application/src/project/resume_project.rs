use pwf_models::project::{HomeDirectory, ProjectId};
use pwf_wire::project::ProjectStateChange;

use super::TaskLocationError;
use crate::ports::project_store::ProjectStore;

#[derive(Debug, thiserror::Error)]
pub enum ResumeProjectError {
    #[error("project not found: {id}")]
    ProjectNotFound { id: ProjectId },
    #[error(transparent)]
    TaskLocation(#[from] TaskLocationError),
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
    home: &HomeDirectory,
) -> Result<ProjectStateChange, ResumeProjectError> {
    projects.resume_project(project_id, home).await
}
