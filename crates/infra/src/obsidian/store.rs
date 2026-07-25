mod add;
mod error;
mod fs;
mod handoff_document;
mod handoff_ledger;
mod index_entry;
mod item_record;
mod lookup;
mod read;
mod read_parser;
#[cfg(test)]
mod tests;

use std::path::Path;

pub use error::ObsidianStoreError;
use pwf_domain::pending_work::ProjectName;

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

    pub fn tasks_path(&self, project: &ProjectName) -> Result<&Path, ObsidianStoreError> {
        self.project_paths.project_directory(project)
    }
}
