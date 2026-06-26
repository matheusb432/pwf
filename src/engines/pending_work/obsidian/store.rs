use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub(in crate::engines::pending_work) enum StoreError {
    #[error("Cannot create project dir: {source}")]
    CreateProjectDir { source: std::io::Error },
    #[error("Cannot create index dir: {source}")]
    CreateIndexDir { source: std::io::Error },
    #[error("Cannot create archive dir: {source}")]
    CreateArchiveDir { source: std::io::Error },
    #[error("Cannot read item file: {source}")]
    ReadItemFile { source: std::io::Error },
    #[error("Cannot read index: {source}")]
    ReadIndex { source: std::io::Error },
    #[error("Cannot read note: {source}")]
    ReadNote { source: std::io::Error },
    #[error("Cannot write item file: {source}")]
    WriteItemFile { source: std::io::Error },
    #[error("Cannot write index: {source}")]
    WriteIndex { source: std::io::Error },
    #[error("Failed to write item file: {source}")]
    AddWriteItemFile { source: std::io::Error },
    #[error("Failed to write index file: {source}")]
    AddWriteIndexFile { source: std::io::Error },
    #[error("Cannot write note: {source}")]
    WriteNote { source: std::io::Error },
    #[error("Cannot remove item file: {source}")]
    RemoveItemFile { source: std::io::Error },
    #[error("Cannot archive {id}: {source}")]
    ArchiveItem { id: String, source: std::io::Error },
}

pub(in crate::engines::pending_work) struct ObsidianStore;

impl ObsidianStore {
    pub(in crate::engines::pending_work) fn create_project_dir(
        path: &Path,
    ) -> Result<(), StoreError> {
        std::fs::create_dir_all(path).map_err(|source| StoreError::CreateProjectDir { source })
    }

    pub(in crate::engines::pending_work) fn create_index_dir(
        path: &Path,
    ) -> Result<(), StoreError> {
        std::fs::create_dir_all(path).map_err(|source| StoreError::CreateIndexDir { source })
    }

    pub(in crate::engines::pending_work) fn read_item_file(
        path: &Path,
    ) -> Result<String, StoreError> {
        std::fs::read_to_string(path).map_err(|source| StoreError::ReadItemFile { source })
    }

    pub(in crate::engines::pending_work) fn read_index(path: &Path) -> Result<String, StoreError> {
        std::fs::read_to_string(path).map_err(|source| StoreError::ReadIndex { source })
    }

    pub(in crate::engines::pending_work) fn read_note(path: &Path) -> Result<String, StoreError> {
        std::fs::read_to_string(path).map_err(|source| StoreError::ReadNote { source })
    }

    pub(in crate::engines::pending_work) fn read_text_or_default(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    pub(in crate::engines::pending_work) fn read_text_optional(path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    pub(in crate::engines::pending_work) fn write_item_file(
        path: &Path,
        content: &str,
    ) -> Result<(), StoreError> {
        crate::fs_atomic::write_text_atomic(path, content)
            .map_err(|source| StoreError::WriteItemFile { source })
    }

    pub(in crate::engines::pending_work) fn write_index(
        path: &Path,
        content: &str,
    ) -> Result<(), StoreError> {
        crate::fs_atomic::write_text_atomic(path, content)
            .map_err(|source| StoreError::WriteIndex { source })
    }

    pub(in crate::engines::pending_work) fn write_add_item_file(
        path: &Path,
        content: &str,
    ) -> Result<(), StoreError> {
        crate::fs_atomic::write_text_atomic(path, content)
            .map_err(|source| StoreError::AddWriteItemFile { source })
    }

    pub(in crate::engines::pending_work) fn write_add_index_file(
        path: &Path,
        content: &str,
    ) -> Result<(), StoreError> {
        crate::fs_atomic::write_text_atomic(path, content)
            .map_err(|source| StoreError::AddWriteIndexFile { source })
    }

    pub(in crate::engines::pending_work) fn write_note(
        path: &Path,
        content: &str,
    ) -> Result<(), StoreError> {
        crate::fs_atomic::write_text_atomic(path, content)
            .map_err(|source| StoreError::WriteNote { source })
    }

    pub(in crate::engines::pending_work) fn remove_item_file(
        path: &Path,
    ) -> Result<(), StoreError> {
        std::fs::remove_file(path).map_err(|source| StoreError::RemoveItemFile { source })
    }

    pub(in crate::engines::pending_work) fn archive_item_file(
        project_dir: &Path,
        id: &str,
    ) -> Result<(), StoreError> {
        let src = project_dir.join(format!("{id}.md"));
        if !src.exists() {
            return Ok(());
        }
        let archive_dir = project_dir.join(super::super::naming::ARCHIVE_DIR);
        std::fs::create_dir_all(&archive_dir)
            .map_err(|source| StoreError::CreateArchiveDir { source })?;
        std::fs::rename(&src, archive_dir.join(format!("{id}.md"))).map_err(|source| {
            StoreError::ArchiveItem {
                id: id.to_string(),
                source,
            }
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    fn missing_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("pwf_store_{name}_{}", nanos()))
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let path = missing_path(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    fn assert_has_source(err: &StoreError) {
        assert!(std::error::Error::source(err).is_some());
    }

    #[test]
    fn create_project_dir_error_is_matchable_and_preserves_text() {
        let parent_file = missing_path("create_project_parent_file");
        std::fs::write(&parent_file, "not a dir").unwrap();

        let err = ObsidianStore::create_project_dir(&parent_file.join("child")).unwrap_err();

        assert_matches!(err, StoreError::CreateProjectDir { .. });
        assert_has_source(&err);
        assert!(err.to_string().starts_with("Cannot create project dir: "));
    }

    #[test]
    fn read_item_file_error_is_matchable_and_preserves_text() {
        let err = ObsidianStore::read_item_file(&missing_path("read_item_file")).unwrap_err();

        assert_matches!(err, StoreError::ReadItemFile { .. });
        assert_has_source(&err);
        assert!(err.to_string().starts_with("Cannot read item file: "));
    }

    #[test]
    fn write_item_file_error_is_matchable_and_preserves_text() {
        let dir = temp_dir("write_item_file");

        let err = ObsidianStore::write_item_file(&dir, "content").unwrap_err();

        assert_matches!(err, StoreError::WriteItemFile { .. });
        assert_has_source(&err);
        assert!(err.to_string().starts_with("Cannot write item file: "));
    }

    #[test]
    fn add_write_item_file_error_is_matchable_and_preserves_text() {
        let dir = temp_dir("add_write_item_file");

        let err = ObsidianStore::write_add_item_file(&dir, "content").unwrap_err();

        assert_matches!(err, StoreError::AddWriteItemFile { .. });
        assert_has_source(&err);
        assert!(err.to_string().starts_with("Failed to write item file: "));
    }

    #[test]
    fn read_index_error_is_matchable_and_preserves_text() {
        let err = ObsidianStore::read_index(&missing_path("read_index")).unwrap_err();

        assert_matches!(err, StoreError::ReadIndex { .. });
        assert_has_source(&err);
        assert!(err.to_string().starts_with("Cannot read index: "));
    }

    #[test]
    fn write_index_error_is_matchable_and_preserves_text() {
        let dir = temp_dir("write_index");

        let err = ObsidianStore::write_index(&dir, "content").unwrap_err();

        assert_matches!(err, StoreError::WriteIndex { .. });
        assert_has_source(&err);
        assert!(err.to_string().starts_with("Cannot write index: "));
    }

    #[test]
    fn add_write_index_file_error_is_matchable_and_preserves_text() {
        let dir = temp_dir("add_write_index_file");

        let err = ObsidianStore::write_add_index_file(&dir, "content").unwrap_err();

        assert_matches!(err, StoreError::AddWriteIndexFile { .. });
        assert_has_source(&err);
        assert!(err.to_string().starts_with("Failed to write index file: "));
    }

    #[test]
    fn archive_item_error_is_matchable_and_preserves_text() {
        let project_dir = temp_dir("archive_item");
        std::fs::write(project_dir.join("GLP-0001.md"), "content").unwrap();
        std::fs::create_dir_all(project_dir.join("_archive/GLP-0001.md")).unwrap();

        let err = ObsidianStore::archive_item_file(&project_dir, "GLP-0001").unwrap_err();

        assert_matches!(
            err,
            StoreError::ArchiveItem { ref id, .. } if id == "GLP-0001"
        );
        assert_has_source(&err);
        assert!(err.to_string().starts_with("Cannot archive GLP-0001: "));
    }
}
