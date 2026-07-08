use std::path::{Path, PathBuf};

use super::ObsidianPendingWorkStoreError;

pub(super) const ARCHIVE_DIR: &str = "_archive";

pub(super) fn path_str(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(super) fn project_archive_dir(notes_dir: &str, name: &str) -> PathBuf {
    pwf_core::paths::project_dir(notes_dir, name).join(ARCHIVE_DIR)
}

pub(super) fn read_text_optional(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

pub(super) fn read_text_or_default(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

pub(super) fn read_item_file(path: &Path) -> Result<String, ObsidianPendingWorkStoreError> {
    std::fs::read_to_string(path)
        .map_err(|source| ObsidianPendingWorkStoreError::ReadItemFile { source })
}

pub(super) fn read_index(path: &Path) -> Result<String, ObsidianPendingWorkStoreError> {
    std::fs::read_to_string(path)
        .map_err(|source| ObsidianPendingWorkStoreError::ReadIndex { source })
}

pub(super) fn write_item_file(
    path: &Path,
    content: &str,
) -> Result<(), ObsidianPendingWorkStoreError> {
    pwf_core::fs_atomic::write_text_atomic(path, content)
        .map_err(|source| ObsidianPendingWorkStoreError::WriteItemFile { source })
}

pub(super) fn write_index(path: &Path, content: &str) -> Result<(), ObsidianPendingWorkStoreError> {
    pwf_core::fs_atomic::write_text_atomic(path, content)
        .map_err(|source| ObsidianPendingWorkStoreError::WriteIndex { source })
}

pub(super) fn remove_item_file(path: &Path) -> Result<(), ObsidianPendingWorkStoreError> {
    std::fs::remove_file(path)
        .map_err(|source| ObsidianPendingWorkStoreError::RemoveItemFile { source })
}

pub(super) fn archive_item_file(
    project_dir: &Path,
    id: &str,
) -> Result<(), ObsidianPendingWorkStoreError> {
    let src = project_dir.join(format!("{id}.md"));
    if !src.exists() {
        return Ok(());
    }
    let archive_dir = project_dir.join(ARCHIVE_DIR);
    std::fs::create_dir_all(&archive_dir)
        .map_err(|source| ObsidianPendingWorkStoreError::CreateArchiveDir { source })?;
    std::fs::rename(&src, archive_dir.join(format!("{id}.md"))).map_err(|source| {
        ObsidianPendingWorkStoreError::ArchiveItem {
            id: id.to_string(),
            source,
        }
    })
}

pub(super) fn line_start_index(content: &str, line_number: usize) -> Option<usize> {
    if line_number == 0 {
        return None;
    }
    if line_number == 1 {
        return Some(0);
    }
    let mut current_line = 1;
    for (index, byte) in content.bytes().enumerate() {
        if byte == b'\n' {
            current_line += 1;
            if current_line == line_number {
                return Some(index + 1);
            }
        }
    }
    None
}

pub(super) fn write_add_item_file(
    path: &Path,
    content: &str,
) -> Result<(), ObsidianPendingWorkStoreError> {
    pwf_core::fs_atomic::write_text_atomic(path, content)
        .map_err(|source| ObsidianPendingWorkStoreError::AddWriteItemFile { source })
}

pub(super) fn write_add_index_file(
    path: &Path,
    content: &str,
    project: &str,
    created_section: Option<&str>,
) -> Result<(), ObsidianPendingWorkStoreError> {
    pwf_core::fs_atomic::write_text_atomic(path, content).map_err(|source| {
        ObsidianPendingWorkStoreError::AddWriteIndexFile {
            source,
            project: project.to_string(),
            created_section: created_section.map(str::to_string),
        }
    })
}
