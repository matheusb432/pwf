use std::path::PathBuf;

use pwf_models::pending_work::{ProjectName, WorkItemId};

use super::{ObsidianStore, ObsidianStoreError};
use crate::obsidian::identity::inspect_project_task_notes;

pub(super) struct TaskFile {
    pub(super) id: WorkItemId,
    pub(super) path: PathBuf,
    pub(super) markdown: String,
    pub(super) title: Option<String>,
}

impl ObsidianStore {
    pub(super) fn task_files_for_project(
        &self,
        project: &ProjectName,
    ) -> Result<Vec<TaskFile>, ObsidianStoreError> {
        let project_dir = self.project_paths.project_directory(project)?;
        let index_path = self.project_paths.project_index_path(project)?;
        if !project_dir.exists() {
            return Ok(Vec::new());
        }
        let identity = self.project_paths.project_identity(project)?;
        inspect_project_task_notes(project_dir, &index_path, identity).map(|tasks| {
            tasks
                .into_iter()
                .map(|task| TaskFile {
                    id: task.id,
                    path: task.path,
                    markdown: task.markdown,
                    title: task.title,
                })
                .collect()
        })
    }

    pub(super) fn next_task_id(
        &self,
        project: &ProjectName,
        prefix: &str,
    ) -> Result<String, ObsidianStoreError> {
        let maximum = self
            .task_files_for_project(project)?
            .into_iter()
            .filter_map(|task| {
                task.id
                    .as_ref()
                    .split_once('-')
                    .and_then(|(_, number)| number.parse::<u32>().ok())
            })
            .max()
            .unwrap_or(0);
        Ok(format!("{prefix}-{:04}", maximum + 1))
    }
}
