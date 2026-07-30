use std::path::PathBuf;

use pwf_application::pending_work::note_body;
use pwf_models::pending_work::{EffortTier, ProjectName, Tags, TaskTitle};

use super::{ObsidianStore, ObsidianStoreError, fs::write_add_item_file};
use crate::obsidian::note_frontmatter::{NewWorkItemFields, new_work_item_content};

/// Contains note-file fields independently of index linking.
pub(super) struct NewNoteRequest<'a> {
    pub prompt: &'a str,
    pub title: &'a TaskTitle,
    pub created: &'a str,
    pub prereq: Option<&'a str>,
    pub effort: Option<EffortTier>,
    pub tags: Option<&'a Tags>,
}

/// Contains a written task note's identity, path, title, and exact bytes.
pub(super) struct WrittenNote {
    pub id: String,
    pub path: PathBuf,
    pub title: TaskTitle,
    pub content: String,
}

impl ObsidianStore {
    /// Allocates and writes a task note without adding an index link.
    pub(super) fn write_new_note(
        &self,
        project: &ProjectName,
        prefix: &str,
        request: &NewNoteRequest<'_>,
    ) -> Result<WrittenNote, ObsidianStoreError> {
        let dir = self.project_paths.project_directory(project)?;
        if !dir.exists() {
            std::fs::create_dir_all(dir)
                .map_err(|source| ObsidianStoreError::CreateProjectDir { source })?;
        }
        let id = self.next_task_id(project, prefix)?;
        let path = dir.join(format!("{id}.md"));
        let body = note_body(request.prompt);
        let content = new_work_item_content(NewWorkItemFields {
            id: &id,
            title: request.title,
            project: project.as_ref(),
            prompt: &body,
            created: request.created,
            prereq: request.prereq,
            effort: request.effort,
            tags: request.tags,
        });
        write_add_item_file(&path, &content)?;
        Ok(WrittenNote {
            id,
            path,
            title: request.title.clone(),
            content,
        })
    }
}
