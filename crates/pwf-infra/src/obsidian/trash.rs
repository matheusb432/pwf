use std::{fs, io, path::Path};

use super::ObsidianStoreError;

pub(super) fn task_file_trash_destination(
    task_path: &Path,
    trash_directory: &Path,
) -> Result<std::path::PathBuf, ObsidianStoreError> {
    require_trash_directory(trash_directory)?;
    let file_name =
        task_path
            .file_name()
            .ok_or_else(|| ObsidianStoreError::TaskFileNameMissing {
                path: task_path.to_path_buf(),
            })?;
    let entry_count = trash_entry_count(trash_directory)
        .map_err(|source| ObsidianStoreError::TaskMutationFilesystem { source })?;
    let mut name = file_name.to_os_string();
    let stem = task_path
        .file_stem()
        .ok_or_else(|| ObsidianStoreError::TaskFileNameMissing {
            path: task_path.to_path_buf(),
        })?;
    for suffix in 1..=entry_count {
        if !path_occupied(&trash_directory.join(&name))
            .map_err(|source| ObsidianStoreError::TaskMutationFilesystem { source })?
        {
            break;
        }
        name = stem.to_os_string();
        name.push(format!(" ({suffix})"));
        if let Some(extension) = task_path.extension() {
            name.push(".");
            name.push(extension);
        }
    }
    Ok(trash_directory.join(name))
}

pub(super) fn require_trash_directory(directory: &Path) -> Result<(), ObsidianStoreError> {
    if !directory.is_dir() {
        return Err(ObsidianStoreError::TaskTrashDirectory {
            path: directory.to_path_buf(),
        });
    }
    Ok(())
}

fn trash_entry_count(directory: &Path) -> io::Result<usize> {
    fs::read_dir(directory)?.map(|entry| entry.map(|_| 1)).sum()
}

fn path_occupied(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trash_names_skip_directories_and_use_the_first_available_suffix() {
        let vault = tempfile::tempdir().unwrap();
        fs::create_dir(vault.path().join(".obsidian")).unwrap();
        let trash = vault.path().join(".trash");
        fs::create_dir_all(trash.join("PWF-0208.md")).unwrap();
        for name in ["PWF-0208 (1).md", "PWF-0208 (3).md"] {
            fs::write(trash.join(name), "previously removed task").unwrap();
        }

        let destination =
            task_file_trash_destination(&vault.path().join("PWF-0208.md"), &trash).unwrap();

        assert_eq!(destination, trash.join("PWF-0208 (2).md"));
    }
}
