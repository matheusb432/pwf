mod add;
#[cfg(test)]
mod contract;
mod error;
mod lookup;
mod project_note;
mod project_snapshot;
mod task_mutation;
mod task_record;

use std::path::PathBuf;

pub use error::ObsidianStoreError;
use pwf_application::ports::project_task_location::ProjectTaskLocationClient;
use pwf_models::project::{HomeDirectory, Project};
use pwf_wire::task::ProjectTaskPath;

#[derive(Clone)]
pub struct ObsidianStore {
    home: HomeDirectory,
}

impl ObsidianStore {
    #[must_use]
    pub fn new(home: HomeDirectory) -> Self {
        Self { home }
    }

    fn tasks_path(&self, project: &Project) -> Result<PathBuf, ObsidianStoreError> {
        pwf_application::project::runtime_path::resolve(project.tasks.path().as_ref(), &self.home)
            .map(|resolved| resolved.path().to_path_buf())
            .map_err(|source| ObsidianStoreError::InvalidProjectTaskPath {
                project: project.title.to_string(),
                source,
            })
    }

    fn project_page_path(&self, project: &Project) -> Result<PathBuf, ObsidianStoreError> {
        Ok(self
            .tasks_path(project)?
            .join(format!("{}.md", project.title)))
    }
}

impl ProjectTaskLocationClient for ObsidianStore {
    type Error = ObsidianStoreError;

    fn project_task_path(&self, project: &Project) -> Result<ProjectTaskPath, Self::Error> {
        self.tasks_path(project).map(ProjectTaskPath::new)
    }
}
