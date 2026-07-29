use std::path::{Path, PathBuf};

use pwf_application::NoteMarkdownSource;
use pwf_models::pending_work::ProjectName;

use super::{
    ObsidianStore, ObsidianStoreError,
    fs::{read_item_file, read_text_optional},
};
use crate::obsidian::identity::{parse_project_index_identity, validate_project_index_identity};

impl ObsidianStore {
    /// Reads a project index only after validating its identity frontmatter.
    pub(super) fn validated_project_index(
        &self,
        project: &ProjectName,
    ) -> Result<Option<(PathBuf, String)>, ObsidianStoreError> {
        let index_path = self.project_paths.project_index_path(project)?;
        let Some(text) = read_text_optional(&index_path) else {
            return Ok(None);
        };
        let actual = parse_project_index_identity(&index_path, &text)?;
        let expected = self.project_paths.project_identity(project)?;
        validate_project_index_identity(&index_path, &actual, expected)?;
        Ok(Some((index_path, text)))
    }
}

impl NoteMarkdownSource for ObsidianStore {
    type Error = ObsidianStoreError;

    fn read_note_markdown(&self, path: &Path) -> Result<String, Self::Error> {
        read_item_file(path)
    }
}
