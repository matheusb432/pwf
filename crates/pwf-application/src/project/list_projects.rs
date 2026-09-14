use pwf_models::project::Project;
use pwf_wire::project::ProjectStatusFilter;

use crate::ports::project_store::ProjectStore;

#[derive(Debug, thiserror::Error)]
pub enum ListProjectsError {
    #[error("{context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: anyhow::Error,
    },
}

#[cqrsy::query]
pub async fn execute(
    status: ProjectStatusFilter,
    projects: &impl ProjectStore,
) -> Result<Vec<Project>, ListProjectsError> {
    projects.list_projects(status).await
}
