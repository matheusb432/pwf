use std::path::PathBuf;

use pwf_models::project::Project;

use super::{ObsidianStore, ObsidianStoreError, fs::read_text_optional};
use crate::obsidian::{
    MarkdownFile,
    identity::{parse_project_index_identity, validate_project_index_identity},
};

impl ObsidianStore {
    /// Reads a project index only after validating its identity frontmatter.
    pub(super) fn validated_project_index(
        &self,
        project: &Project,
    ) -> Result<Option<(PathBuf, String)>, ObsidianStoreError> {
        let index_path = self.project_index_path(project)?;
        let Some(text) = read_text_optional(&index_path) else {
            return Ok(None);
        };
        let file = MarkdownFile::from_source(index_path.clone(), text);
        let actual = parse_project_index_identity(&file)?;
        let expected = Self::project_identity(project);
        validate_project_index_identity(&index_path, &actual, &expected)?;
        Ok(Some((index_path, file.into_source())))
    }
}
