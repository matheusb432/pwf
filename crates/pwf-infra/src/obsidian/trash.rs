use std::path::Path;

use super::ObsidianStoreError;

pub(super) fn move_task_file_to_vault_trash(task_path: &Path) -> Result<(), ObsidianStoreError> {
    let vault_root = task_path
        .parent()
        .and_then(|directory| {
            directory
                .ancestors()
                .find(|ancestor| ancestor.join(".obsidian").is_dir())
        })
        .ok_or_else(|| ObsidianStoreError::TaskVaultNotFound {
            path: task_path.to_path_buf(),
        })?;
    let trash_directory = vault_root.join(".trash");
    std::fs::create_dir_all(&trash_directory).map_err(|source| {
        ObsidianStoreError::CreateTaskTrashDirectory {
            path: trash_directory.clone(),
            source,
        }
    })?;
    let file_name =
        task_path
            .file_name()
            .ok_or_else(|| ObsidianStoreError::TaskFileNameMissing {
                path: task_path.to_path_buf(),
            })?;
    let destination = trash_directory.join(file_name);
    let destination_exists = destination.try_exists().map_err(|source| {
        ObsidianStoreError::InspectTaskTrashDestination {
            path: destination.clone(),
            source,
        }
    })?;
    if destination_exists {
        return Err(ObsidianStoreError::TaskTrashDestinationExists { path: destination });
    }
    std::fs::rename(task_path, &destination).map_err(|source| {
        ObsidianStoreError::MoveTaskFileToTrash {
            from: task_path.to_path_buf(),
            to: destination,
            source,
        }
    })
}
