use std::collections::BTreeSet;

use pwf_models::project::{Project, ProjectId};

use super::get_project::GetProjectError;
use crate::ports::project_store::ProjectStore;

#[cqrsy::query]
pub async fn execute(
    ids: &BTreeSet<ProjectId>,
    projects: &impl ProjectStore,
) -> Result<Vec<Project>, GetProjectError> {
    projects.get_projects(ids).await
}
