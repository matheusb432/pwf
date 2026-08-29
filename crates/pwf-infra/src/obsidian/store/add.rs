use std::path::PathBuf;

use pwf_models::{
    project::Project,
    task::{BlockedBy, EffortTier, TaskId, TaskTags, TaskTimestamp, TaskTitle},
};

use super::{ObsidianStore, ObsidianStoreError, fs::write_add_task_file};
use crate::obsidian::note_frontmatter::{NewTaskFields, new_task_content};

/// Contains note-file fields independently of index linking.
pub(super) struct NewNoteRequest<'a> {
    pub id: &'a TaskId,
    pub body: &'a str,
    pub title: &'a TaskTitle,
    pub created_at: &'a TaskTimestamp,
    pub blocked_by: Option<&'a BlockedBy>,
    pub effort: Option<EffortTier>,
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
    /// Writes an exact task ID without adding an index link.
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
        let content = new_task_content(NewTaskFields {
            id: request.id,
            title: request.title,
            project: project.title.as_ref(),
            body: request.body,
            created_at: request.created_at,
            blocked_by: request.blocked_by,
            effort: request.effort,
            tags: request.tags,
        });
        write_add_task_file(&path, &content)
            .map_err(|error| map_add_task_error(error, request.id, &path))?;
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
