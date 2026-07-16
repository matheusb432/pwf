use std::path::{Path, PathBuf};

use pwf_application::NoteMarkdownSource;
use pwf_domain::pending_work::ProjectName;

use super::{
    ObsidianStore, ObsidianStoreError,
    fs::{read_item_file, read_text_optional},
};
use crate::obsidian::identity::{
    configured_project_index_identity, parse_project_index_identity,
    validate_project_index_identity,
};

impl ObsidianStore {
    /// Reads and validates a project index's identity frontmatter, returning its
    /// `(path, text)` when present. Shared by the generic-port read
    /// materialization (`get`/`list`) so index-identity rejection is enforced on
    /// every read path.
    pub(super) fn validated_project_index(
        &self,
        project: &ProjectName,
    ) -> Result<Option<(PathBuf, String)>, ObsidianStoreError> {
        let index_path = pwf_core::paths::project_index_path(
            self.config.notes_dir_for(project.as_ref()),
            project.as_ref(),
        );
        let Some(text) = read_text_optional(&index_path) else {
            return Ok(None);
        };
        let actual = parse_project_index_identity(&index_path, &text)?;
        let expected = configured_project_index_identity(&self.config, project)?;
        validate_project_index_identity(&index_path, &actual, &expected)?;
        Ok(Some((index_path, text)))
    }
}

impl NoteMarkdownSource for ObsidianStore {
    type Error = ObsidianStoreError;

    fn read_note_markdown(&self, path: &Path) -> Result<String, Self::Error> {
        read_item_file(path)
    }
}
