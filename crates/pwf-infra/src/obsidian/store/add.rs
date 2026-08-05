use std::path::PathBuf;

use pwf_models::{
    project::Project,
    task::{EffortTier, Prerequisites, Tags, TaskId, TaskTitle},
};

use super::{ObsidianStore, ObsidianStoreError, fs::write_add_task_file};
use crate::obsidian::note_frontmatter::{NewTaskFields, new_task_content};

/// Contains note-file fields independently of index linking.
pub(super) struct NewNoteRequest<'a> {
    pub body: &'a str,
    pub title: &'a TaskTitle,
    pub created: &'a str,
    pub prereq: Option<&'a Prerequisites>,
    pub effort: Option<EffortTier>,
    pub tags: Option<&'a Tags>,
}

/// Contains a written task note's identity, path, title, and exact bytes.
pub(super) struct WrittenNote {
    pub id: TaskId,
    pub path: PathBuf,
    pub title: TaskTitle,
    pub content: String,
}

impl ObsidianStore {
    /// Allocates and writes a task note without adding an index link.
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
        let id = self.next_task_id(project)?;
        let path = dir.join(format!("{id}.md"));
        let content = new_task_content(NewTaskFields {
            id: &id,
            title: request.title,
            project: project.title.as_ref(),
            prompt: request.body,
            created: request.created,
            prereq: request.prereq,
            effort: request.effort,
            tags: request.tags,
        });
        write_add_task_file(&path, &content)?;
        Ok(WrittenNote {
            id,
            path,
            title: request.title.clone(),
            content,
        })
    }
}
