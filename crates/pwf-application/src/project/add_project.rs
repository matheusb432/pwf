use pwf_models::project::{HomeDirectory, Project, ProjectId, ProjectName};
use pwf_wire::project::ProjectFields;

use super::TaskLocationError;
use crate::ports::project_store::ProjectStore;

#[derive(Debug, thiserror::Error)]
pub enum AddProjectError {
    #[error("project id already exists: {id}")]
    DuplicateProjectId { id: ProjectId },
    #[error("project title already exists: {title}")]
    DuplicateProjectTitle { title: ProjectName },
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
    fields: ProjectFields,
    projects: &impl ProjectStore,
    home: &HomeDirectory,
) -> Result<Project, AddProjectError> {
    projects.add_project(fields, home).await
}
