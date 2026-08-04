mod add;
#[cfg(test)]
mod contract;
mod error;
mod fs;
mod index_entry;
mod lookup;
mod project_note;
mod read;
mod task_record;

use std::path::PathBuf;

pub use error::ObsidianStoreError;
use pwf_application::ports::project_task_location::ProjectTaskLocationClient;
use pwf_models::project::{Project, ProjectIndexIdentity};

#[derive(Clone)]
pub struct ObsidianStore {
    home: PathBuf,
}

impl ObsidianStore {
    pub fn new(home: PathBuf) -> Self {
        Self { home }
    }

    fn tasks_path(&self, project: &Project) -> Result<PathBuf, ObsidianStoreError> {
        pwf_application::project::resolve_runtime_path::execute(
            &pwf_application::project::resolve_runtime_path::ResolveRuntimePath {
                path: project.tasks.path().to_string(),
                home: self.home.clone(),
            },
        )
        .map(|resolved| resolved.path().to_path_buf())
        .map_err(|source| ObsidianStoreError::InvalidProjectTaskPath {
            project: project.title.to_string(),
            source,
        })
    }

    fn project_index_path(&self, project: &Project) -> Result<PathBuf, ObsidianStoreError> {
        Ok(self
            .tasks_path(project)?
            .join(format!("{}.md", project.title)))
    }

    fn project_identity(project: &Project) -> ProjectIndexIdentity {
        ProjectIndexIdentity::new(project.id.clone(), project.title.clone())
    }
}

impl ProjectTaskLocationClient for ObsidianStore {
    type Error = ObsidianStoreError;

    fn project_task_path(&self, project: &Project) -> Result<PathBuf, Self::Error> {
        self.tasks_path(project)
    }
}
