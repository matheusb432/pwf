use std::error::Error;

use pwf_models::project::Project;

use crate::contract::task::ProjectTaskPath;

/// Resolves one managed project's runtime task directory.
pub trait ProjectTaskLocationClient: Clone + Send + Sync + 'static {
    /// Concrete adapter failure.
    type Error: Error + Send + Sync + 'static;

    /// Returns the runtime task directory for `project`.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the project has no configured task directory.
    fn project_task_path(&self, project: &Project) -> Result<ProjectTaskPath, Self::Error>;
}
