use std::path::PathBuf;

use pwf_domain::pending_work::{ProjectName, WorkItemId};

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
        let project_dir = pwf_core::paths::project_dir(
            self.config.notes_dir_for(project.as_ref()),
            project.as_ref(),
        );
        let index_path = pwf_core::paths::project_index_path(
            self.config.notes_dir_for(project.as_ref()),
            project.as_ref(),
        );
        if !project_dir.exists() {
            return Ok(Vec::new());
        }
        let prefix = self.config.prefixes.get(project.as_ref()).ok_or_else(|| {
            ObsidianStoreError::ProjectMissingPrefix {
                project: project.as_ref().to_string(),
            }
        })?;
        inspect_project_task_notes(&project_dir, &index_path, prefix, project.as_ref()).map(
            |tasks| {
                tasks
                    .into_iter()
                    .map(|task| TaskFile {
                        id: task.id,
                        path: task.path,
                        markdown: task.markdown,
                        title: task.title,
                    })
                    .collect()
            },
        )
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
