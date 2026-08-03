use std::path::Path;

use super::ObsidianStoreError;

pub(super) fn path_str(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(super) fn read_text_optional(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

pub(super) fn read_task_file(path: &Path) -> Result<String, ObsidianStoreError> {
    std::fs::read_to_string(path).map_err(|source| ObsidianStoreError::ReadTaskFile { source })
}

pub(super) fn read_index(path: &Path) -> Result<String, ObsidianStoreError> {
    std::fs::read_to_string(path).map_err(|source| ObsidianStoreError::ReadIndex { source })
}

pub(super) fn write_task_file(path: &Path, content: &str) -> Result<(), ObsidianStoreError> {
    crate::obsidian::fs_atomic::write_text_atomic(path, content)
        .map_err(|source| ObsidianStoreError::WriteTaskFile { source })
}

pub(super) fn write_index(path: &Path, content: &str) -> Result<(), ObsidianStoreError> {
    crate::obsidian::fs_atomic::write_text_atomic(path, content)
        .map_err(|source| ObsidianStoreError::WriteIndex { source })
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

pub(super) fn write_add_task_file(path: &Path, content: &str) -> Result<(), ObsidianStoreError> {
    crate::obsidian::fs_atomic::write_text_atomic(path, content)
        .map_err(|source| ObsidianStoreError::AddWriteTaskFile { source })
}

pub(super) fn write_add_index_file(
    path: &Path,
    content: &str,
    project: &str,
    created_section: Option<&str>,
) -> Result<(), ObsidianStoreError> {
    crate::obsidian::fs_atomic::write_text_atomic(path, content).map_err(|source| {
        ObsidianStoreError::AddWriteIndexFile {
            source,
            project: project.to_string(),
            created_section: created_section.map(str::to_string),
        }
    })
}
