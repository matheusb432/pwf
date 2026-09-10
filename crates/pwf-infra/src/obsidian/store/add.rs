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

/// Contains a written task note's identity, path, title, and exact bytes.
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
        if path == self.project_page_path(project)? {
            return Err(ObsidianStoreError::ProjectPagePathReserved { path });
        }
        if let Some(task) = self
            .map_task_notes(
                project,
                MarkdownFile::read_frontmatter_file,
                |id, _, file, _| Ok((id, file.path().to_path_buf())),
            )?
            .into_iter()
            .map(|(task, _)| task)
            .find(|(id, _)| id == request.id)
        {
            return Err(ObsidianStoreError::TaskIdOccupied {
                id: task.0,
                path: task.1,
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
        MarkdownFile::create_rendered_new(&path, content.clone()).map_err(|source| {
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
