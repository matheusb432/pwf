use pwf_models::{project::Project, task::TaskId};

use super::{ObsidianStore, ObsidianStoreError};
use crate::obsidian::{
    FrontmatterView, MarkdownFile, MarkdownFileError, TaskNoteIdentity,
    identity::{inspect_project_task_notes, map_project_task_notes},
};

impl ObsidianStore {
    pub(super) fn map_task_notes<T>(
        &self,
        project: &Project,
        read: fn(&std::path::Path) -> Result<MarkdownFile, MarkdownFileError>,
        map: impl Fn(
            TaskId,
            Option<String>,
            &MarkdownFile,
            &FrontmatterView<'_>,
        ) -> Result<T, ObsidianStoreError>,
    ) -> Result<Vec<(T, MarkdownFile)>, ObsidianStoreError> {
        let directory = self.tasks_path(project)?;
        if !directory
            .try_exists()
            .map_err(|source| ObsidianStoreError::ReadTaskFile { source })?
        {
            return Ok(Vec::new());
        }
        map_project_task_notes(&directory, &self.project_page_path(project)?, read, map)
    }

    pub(super) fn task_files_for_project(
        &self,
        project: &Project,
    ) -> Result<Vec<TaskNoteIdentity>, ObsidianStoreError> {
        let project_dir = self.tasks_path(project)?;
        let project_page_path = self.project_page_path(project)?;
        if !project_dir
            .try_exists()
            .map_err(|source| ObsidianStoreError::ReadTaskFile { source })?
        {
            return Ok(Vec::new());
        }
        inspect_project_task_notes(&project_dir, &project_page_path)
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
