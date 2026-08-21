use super::{
    Project,
    get_project::{self, GetProjectError},
};
use crate::contract::project::{GetActiveProject, GetProject, ProjectStatusFilter};

/// Reads one active managed project.
#[cqrsy::query]
pub async fn execute(
    query: GetActiveProject,
    pool: &sqlx::SqlitePool,
) -> Result<Project, GetProjectError> {
    get_project::execute(
        GetProject {
            id: query.id,
            status: ProjectStatusFilter::ActiveOnly,
        },
        pool,
    )
    .await
}
