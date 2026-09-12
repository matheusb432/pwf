use std::sync::Arc;

use pwf_application::ports::task_vault::TaskGraphRecord;
use pwf_models::{project::Project, task::TaskId};

use super::{
    ObsidianStore, ObsidianStoreError,
    task_record::{task_graph_metadata, task_summary_metadata},
};
use crate::obsidian::{
    FrontmatterView, MarkdownFile,
    identity::{TaskFile, TaskFileIdentity, TaskRead, map_project_task_files},
    note_frontmatter::parse_blocked_by,
};

impl ObsidianStore {
    pub(super) fn map_task_files<T>(
        &self,
        project: &Project,
        read: TaskRead,
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
            self.invalidate_task_index(&directory);
            return Ok(Vec::new());
        }
        map_project_task_files(&directory, read, map)
    }

    pub(super) fn task_files_indexed(
        &self,
        directory: &std::path::Path,
    ) -> Result<Option<std::sync::Arc<[TaskFile]>>, ObsidianStoreError> {
        let Some(index) = &self.task_index else {
            return Ok(None);
        };
        index
            .read(directory, || {
                map_project_task_files(
                    directory,
                    TaskRead::Frontmatter,
                    |id, title, file, frontmatter| {
                        Ok(task_file_metadata(id, title, file, frontmatter))
                    },
                )
                .map(|notes| notes.into_iter().map(|(note, _)| note).collect())
            })
            .map(Some)
    }

    pub(super) fn task_file_for_project(
        &self,
        project: &Project,
        id: &TaskId,
    ) -> Result<Option<TaskFileIdentity>, ObsidianStoreError> {
        let directory = self.tasks_path(project)?;
        if !directory
            .try_exists()
            .map_err(|source| ObsidianStoreError::ReadTaskFile { source })?
        {
            self.invalidate_task_index(&directory);
            return Ok(None);
        }
        if let Some(notes) = self.task_files_indexed(&directory)? {
            return Ok(notes
                .binary_search_by(|note| note.id.cmp(id))
                .ok()
                .map(|position| TaskFileIdentity {
                    id: notes[position].id.clone(),
                    path: notes[position].path.clone(),
                }));
        }
        self.task_files_for_project(project)
            .map(|notes| notes.into_iter().find(|note| &note.id == id))
    }

    pub(super) fn task_files_for_project(
        &self,
        project: &Project,
    ) -> Result<Vec<TaskFileIdentity>, ObsidianStoreError> {
        self.map_task_files(project, TaskRead::Frontmatter, |id, _, file, _| {
            Ok(TaskFileIdentity {
                id,
                path: file.path().to_path_buf(),
            })
        })
        .map(|notes| notes.into_iter().map(|(identity, _)| identity).collect())
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

fn task_file_metadata(
    id: TaskId,
    title: Option<String>,
    file: &MarkdownFile,
    frontmatter: &FrontmatterView<'_>,
) -> TaskFile {
    // Summary errors must not poison identity or graph reads.
    let summary = task_summary_metadata(id.clone(), title.clone(), file, frontmatter)
        .ok()
        .map(|(summary, _)| summary);
    let graph = summary
        .as_ref()
        .map_or_else(
            || task_graph_metadata(title, file, frontmatter).map_err(Arc::new),
            |summary| {
                Ok(TaskGraphRecord {
                    title: summary.title.clone(),
                    status: summary.status,
                    blocked_by: parse_blocked_by(Some(frontmatter)),
                })
            },
        )
        .map(Arc::new);
    TaskFile {
        id,
        path: file.path().to_path_buf(),
        summary: summary.map(Box::new),
        graph,
    }
}
