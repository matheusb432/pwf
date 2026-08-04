use std::path::PathBuf;

use pwf_models::{project::Project, task::TaskId};

use super::{ObsidianStore, ObsidianStoreError};
use crate::obsidian::identity::inspect_project_task_notes;

pub(super) struct TaskFile {
    pub(super) id: TaskId,
    pub(super) path: PathBuf,
    pub(super) markdown: String,
    pub(super) title: Option<String>,
}

impl ObsidianStore {
    pub(super) fn task_files_for_project(
        &self,
        project: &Project,
    ) -> Result<Vec<TaskFile>, ObsidianStoreError> {
        let project_dir = self.tasks_path(project)?;
        let index_path = self.project_index_path(project)?;
        if !project_dir.exists() {
            return Ok(Vec::new());
        }
        let identity = Self::project_identity(project);
        inspect_project_task_notes(&project_dir, &index_path, &identity).map(|tasks| {
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

    pub(super) fn next_task_id(&self, project: &Project) -> Result<TaskId, ObsidianStoreError> {
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
        TaskId::try_new(format!("{}-{:04}", project.id, maximum + 1)).map_err(|_| {
            ObsidianStoreError::TaskIdSequenceExhausted {
                project_id: project.id.clone(),
            }
        })
    }
}
