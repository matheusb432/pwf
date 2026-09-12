use std::path::PathBuf;

use pwf_application::ports::task_vault::NewTaskBody;
use pwf_models::{
    project::Project,
    task::{BlockedBy, EffortTier, PriorityTier, TaskId, TaskTags, TaskTimestamp, TaskTitle},
};

use super::{ObsidianStore, ObsidianStoreError};
use crate::obsidian::{
    MarkdownFile,
    note_frontmatter::{NewTaskFields, new_task_content},
};

pub(super) struct NewNoteRequest<'a> {
    pub id: &'a TaskId,
    pub body: &'a NewTaskBody,
    pub title: &'a TaskTitle,
    pub created_at: &'a TaskTimestamp,
    pub blocked_by: Option<&'a BlockedBy>,
    pub effort: Option<EffortTier>,
    pub priority: Option<PriorityTier>,
    pub tags: Option<&'a TaskTags>,
}

/// Contains a written task file's identity, path, title, and exact bytes.
pub(super) struct WrittenNote {
    pub id: TaskId,
    pub path: PathBuf,
    pub title: TaskTitle,
    pub content: String,
}

impl ObsidianStore {
    pub(super) fn write_new_note(
        &self,
        project: &Project,
        request: &NewNoteRequest<'_>,
    ) -> Result<WrittenNote, ObsidianStoreError> {
        let dir = self.tasks_path(project)?;
        if !dir.exists() {
            std::fs::create_dir_all(&dir)
                .map_err(|source| ObsidianStoreError::CreateProjectDir { source })?;
        }
        let path = dir.join(format!("{}.md", request.id));
        if self
            .project_snapshot_path(project)?
            .is_some_and(|snapshot_path| path == snapshot_path)
        {
            return Err(ObsidianStoreError::ProjectSnapshotPathReserved { path });
        }
        if let Some(task) = self
            .task_files_for_project(project)?
            .into_iter()
            .find(|task| &task.id == request.id)
        {
            return Err(ObsidianStoreError::TaskIdOccupied {
                id: task.id,
                path: task.path,
            });
        }
        let content = new_task_content(NewTaskFields {
            id: request.id,
            title: request.title,
            project: project.title.as_ref(),
            body: request.body,
            created_at: request.created_at,
            blocked_by: request.blocked_by,
            effort: request.effort,
            priority: request.priority,
            tags: request.tags,
        });
        let result = MarkdownFile::create_rendered_new(&path, content.clone());
        self.invalidate_task_index(&dir);
        result.map_err(|source| {
            map_add_task_error(
                ObsidianStoreError::AddWriteTaskFile {
                    source: source.into_io_error(),
                },
                request.id,
                &path,
            )
        })?;
        Ok(WrittenNote {
            id: request.id.clone(),
            path,
            title: request.title.clone(),
            content,
        })
    }
}

fn map_add_task_error(
    error: ObsidianStoreError,
    id: &TaskId,
    path: &std::path::Path,
) -> ObsidianStoreError {
    match error {
        ObsidianStoreError::AddWriteTaskFile { ref source }
            if source.kind() == std::io::ErrorKind::AlreadyExists =>
        {
            ObsidianStoreError::TaskIdOccupied {
                id: id.clone(),
                path: path.to_path_buf(),
            }
        }
        error => error,
    }
}
