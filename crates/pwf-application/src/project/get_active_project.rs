use pwf_models::project::ProjectId;
use pwf_wire::project::{GetProject, ProjectStatusFilter};

use super::{
    Project,
    get_project::{self, GetProjectError},
};
use crate::ports::project_store::ProjectStore;

/// Reads one active managed project.
#[cqrsy::query]
pub async fn execute(
    id: impl Into<ProjectId>,
    project_store: &impl ProjectStore,
) -> Result<Project, GetProjectError> {
    get_project::execute(
        GetProject::new(id, ProjectStatusFilter::ActiveOnly),
        project_store,
    )
    .await
}
