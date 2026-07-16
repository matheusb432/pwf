use std::path::PathBuf;

use pwf_domain::pending_work::{ProjectName, Tags, inferred_title, normalize_title, note_body};

use super::{ObsidianStore, ObsidianStoreError, fs::write_add_item_file};
use crate::obsidian::note_frontmatter::{NewWorkItemFields, new_work_item_content};

/// The note-file fields a caller wants written, independent of index linking.
pub(super) struct NewNoteRequest<'a> {
    pub prompt: &'a str,
    pub title: Option<&'a str>,
    pub created: &'a str,
    pub prereq: Option<&'a str>,
    pub effort: Option<u8>,
    pub tags: Option<&'a Tags>,
}

/// A freshly written task note: its allocated id, path, resolved title, and the
/// exact bytes written.
pub(super) struct WrittenNote {
    pub id: String,
    pub path: PathBuf,
    pub title: String,
    pub content: String,
}

impl ObsidianStore {
    /// Allocates the next id, builds, and writes a task note — note file only,
    /// no index link. The generic `insert` builds on this and leaves indexing
    /// to the application via the `IndexEntry` upsert.
    pub(super) fn write_new_note(
        &self,
        project: &ProjectName,
        prefix: &str,
        request: &NewNoteRequest<'_>,
    ) -> Result<WrittenNote, ObsidianStoreError> {
        let title = match request.title {
            Some(title) if !title.trim().is_empty() => normalize_title(title),
            _ => inferred_title(request.prompt),
        };
        let dir = pwf_core::paths::project_dir(
            self.config.notes_dir_for(project.as_ref()),
            project.as_ref(),
        );
        if !dir.exists() {
            std::fs::create_dir_all(&dir)
                .map_err(|source| ObsidianStoreError::CreateProjectDir { source })?;
        }
        let id = self.next_task_id(project, prefix)?;
        let path = dir.join(format!("{id}.md"));
        let body = note_body(request.prompt);
        let content = new_work_item_content(NewWorkItemFields {
            id: &id,
            title: &title,
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
            title,
            content,
        })
    }
}
