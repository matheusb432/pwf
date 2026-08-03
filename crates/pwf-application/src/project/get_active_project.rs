use pwf_models::project::ProjectId;

use super::{
    Project, ProjectStatusFilter,
    get_project::{self, GetProject, GetProjectError},
};

/// Requests one active managed project by canonical project ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetActiveProject {
    pub id: ProjectId,
}

/// Reads one active managed project.
#[cqrsy::query]
pub async fn execute(
    query: GetActiveProject,
    pool: &sqlx::SqlitePool,
) -> Result<Project, GetProjectError> {
    get_project::execute(
        GetProject {
            id: query.id,
            status: ProjectStatusFilter::ACTIVE,
        },
        pool,
    )
    .await
}
