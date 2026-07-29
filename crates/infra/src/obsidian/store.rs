mod add;
mod error;
mod fs;
mod index_entry;
mod item_record;
mod lookup;
mod project_note;
mod read;
mod read_parser;
#[cfg(test)]
mod tests;

use std::path::Path;

pub use error::ObsidianStoreError;
use pwf_application::ProjectTaskLocationClient;
use pwf_models::pending_work::ProjectName;

use super::project_paths::{ObsidianProject, ProjectPaths};

#[derive(Clone)]
pub struct ObsidianStore {
    project_paths: ProjectPaths,
}

impl ObsidianStore {
    pub fn new(projects: impl IntoIterator<Item = ObsidianProject>) -> Self {
        Self {
            project_paths: ProjectPaths::from_projects(projects),
        }
    }

    fn tasks_path(&self, project: &ProjectName) -> Result<&Path, ObsidianStoreError> {
        self.project_paths.project_directory(project)
    }
}

impl ProjectTaskLocationClient for ObsidianStore {
    type Error = ObsidianStoreError;

    fn project_task_path(&self, project: &ProjectName) -> Result<std::path::PathBuf, Self::Error> {
        self.tasks_path(project).map(Path::to_path_buf)
    }
}
