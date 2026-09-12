mod add;
#[cfg(test)]
mod contract;
mod error;
mod lookup;
mod project_note;
mod project_snapshot;
mod task_mutation;
mod task_record;

use std::{path::PathBuf, sync::Arc};

pub use error::ObsidianStoreError;
use pwf_application::ports::project_task_location::ProjectTaskLocationClient;
use pwf_models::project::{HomeDirectory, Project};
use pwf_wire::task::ProjectTaskPath;

#[derive(Clone)]
pub struct ObsidianStore {
    home: HomeDirectory,
    task_index: Option<Arc<super::task_index::TaskIndex>>,
}

impl ObsidianStore {
    #[must_use]
    pub fn new(home: HomeDirectory) -> Self {
        Self {
            home,
            task_index: None,
        }
    }

    /// Reuses task summaries and locations between filesystem notifications within a bounded memory
    /// budget. Server writes invalidate immediately; external edits have a short refresh delay.
    #[must_use]
    pub fn with_watched_tasks(home: HomeDirectory) -> Self {
        Self {
            home,
            task_index: Some(Arc::new(super::task_index::TaskIndex::new())),
        }
    }

    fn invalidate_task_index(&self, directory: &std::path::Path) {
        if let Some(index) = &self.task_index {
            index.invalidate(directory);
        }
    }

    fn tasks_path(&self, project: &Project) -> Result<PathBuf, ObsidianStoreError> {
        pwf_application::project::runtime_path::resolve(project.tasks.path().as_ref(), &self.home)
            .map(|resolved| resolved.path().to_path_buf())
            .map_err(|source| ObsidianStoreError::InvalidProjectTaskPath {
                project: project.title.to_string(),
                source,
            })
    }

    fn project_snapshot_path(
        &self,
        project: &Project,
    ) -> Result<Option<PathBuf>, ObsidianStoreError> {
        self.tasks_path(project)
            .map(|directory| super::project_snapshot_path(&directory))
    }
}

impl ProjectTaskLocationClient for ObsidianStore {
    type Error = ObsidianStoreError;

    fn project_task_path(&self, project: &Project) -> Result<ProjectTaskPath, Self::Error> {
        self.tasks_path(project).map(ProjectTaskPath::new)
    }
}
