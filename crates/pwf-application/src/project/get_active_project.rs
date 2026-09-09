use pwf_models::project::ProjectId;
use pwf_wire::project::{GetProject, ProjectStatusFilter};

use super::{
    Project,
    get_project::{self, GetProjectError},
};

/// Reads one active managed project.
#[cqrsy::query]
pub async fn execute(
    id: impl Into<ProjectId>,
    pool: &sqlx::SqlitePool,
) -> Result<Project, GetProjectError> {
    get_project::execute(GetProject::new(id, ProjectStatusFilter::ActiveOnly), pool).await
}
