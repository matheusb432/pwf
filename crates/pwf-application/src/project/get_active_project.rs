use pwf_models::project::ProjectId;
use pwf_wire::project::ProjectStatusFilter;

use super::{
    Project,
    get_project::{self, GetProject, GetProjectError},
};

/// Requests one active managed project by project ID.
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
