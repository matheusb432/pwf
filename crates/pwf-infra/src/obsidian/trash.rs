use std::path::Path;

use super::ObsidianStoreError;

pub(super) fn task_file_trash_destination(
    task_path: &Path,
) -> Result<std::path::PathBuf, ObsidianStoreError> {
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
    let file_name =
        task_path
            .file_name()
            .ok_or_else(|| ObsidianStoreError::TaskFileNameMissing {
                path: task_path.to_path_buf(),
            })?;
    Ok(vault_root.join(".trash").join(file_name))
}
