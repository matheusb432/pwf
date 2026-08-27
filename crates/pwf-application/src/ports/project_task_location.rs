use std::error::Error;

use pwf_models::project::Project;
use pwf_wire::task::ProjectTaskPath;

/// Resolves one managed project's runtime task directory.
pub trait ProjectTaskLocationClient: Clone + Send + Sync + 'static {
    /// Concrete adapter failure.
    type Error: Error + Send + Sync + 'static;

    /// Returns the runtime task directory for `project`.
    fn project_task_path(&self, project: &Project) -> Result<ProjectTaskPath, Self::Error>;
}
