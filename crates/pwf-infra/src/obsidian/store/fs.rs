use std::path::Path;

use pwf_models::task::TaskSection;

use super::ObsidianStoreError;
use crate::obsidian::MarkdownFile;

pub(super) fn path_str(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(super) fn read_text_optional(path: &Path) -> Option<String> {
    MarkdownFile::open(path).ok().map(MarkdownFile::into_source)
}

pub(super) fn read_task_file(path: &Path) -> Result<String, ObsidianStoreError> {
    open_task_file(path).map(MarkdownFile::into_source)
}

pub(super) fn read_index(path: &Path) -> Result<String, ObsidianStoreError> {
    MarkdownFile::open(path)
        .map(MarkdownFile::into_source)
        .map_err(|source| ObsidianStoreError::ReadIndex {
            source: source.into_io_error(),
        })
}

pub(super) fn open_task_file(path: &Path) -> Result<MarkdownFile, ObsidianStoreError> {
    MarkdownFile::open(path).map_err(|source| ObsidianStoreError::ReadTaskFile {
        source: source.into_io_error(),
    })
}

pub(super) fn save_task_file(file: &MarkdownFile) -> Result<(), ObsidianStoreError> {
    file.save()
        .map_err(|source| ObsidianStoreError::WriteTaskFile {
            source: source.into_io_error(),
        })
}

pub(super) fn write_index(path: &Path, content: &str) -> Result<(), ObsidianStoreError> {
    MarkdownFile::write_rendered(path.to_path_buf(), content.to_string()).map_err(|source| {
        ObsidianStoreError::WriteIndex {
            source: source.into_io_error(),
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
        if byte != b'\n' {
            continue;
        }
        current_line += 1;
        if current_line == line_number {
            return Some(index + 1);
        }
    }
    None
}

pub(super) fn write_add_task_file(path: &Path, content: &str) -> Result<(), ObsidianStoreError> {
    MarkdownFile::create_rendered_new(path.to_path_buf(), content.to_string())
        .map(|_| ())
        .map_err(|source| ObsidianStoreError::AddWriteTaskFile {
            source: source.into_io_error(),
        })
}

pub(super) fn write_add_index_file(
    path: &Path,
    content: &str,
    project: &str,
    created_section: Option<&TaskSection>,
) -> Result<(), ObsidianStoreError> {
    MarkdownFile::write_rendered(path.to_path_buf(), content.to_string()).map_err(|source| {
        ObsidianStoreError::AddWriteIndexFile {
            source: source.into_io_error(),
            project: project.to_string(),
            created_section: created_section.cloned(),
        }
    })
}
