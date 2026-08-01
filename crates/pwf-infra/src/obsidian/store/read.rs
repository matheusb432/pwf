use std::path::PathBuf;

use pwf_models::project::Project;

use super::{ObsidianStore, ObsidianStoreError, fs::read_text_optional};
use crate::obsidian::identity::{parse_project_index_identity, validate_project_index_identity};

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
        let actual = parse_project_index_identity(&index_path, &text)?;
        let expected = Self::project_identity(project);
        validate_project_index_identity(&index_path, &actual, &expected)?;
        Ok(Some((index_path, text)))
    }
}
