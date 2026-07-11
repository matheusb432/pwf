use std::path::Path;

use pwf_domain::pending_work::RemovedItem;

use super::{
    ObsidianPendingWorkStore, ObsidianPendingWorkStoreError,
    fs::{read_index, write_index},
};
use crate::obsidian::index_text::remove_index_link;

impl ObsidianPendingWorkStore {
    pub(super) fn remove_item_impl(
        &self,
        id: &str,
    ) -> Result<RemovedItem, ObsidianPendingWorkStoreError> {
        let item = self.find_pending_item(id)?;
        let item_file = item
            .item_file
            .as_deref()
            .ok_or(ObsidianPendingWorkStoreError::RemoveRequiresFileModel)?;
        let item_path = Path::new(item_file);
        if !item_path.exists() {
            return Err(ObsidianPendingWorkStoreError::WorkItemNoteMissing {
                path: item_path.to_path_buf(),
            });
        }

        let index_path = pwf_core::paths::project_index_path(
            self.config.notes_dir_for(&item.project),
            &item.project,
        );
        let index_content = read_index(&index_path)?;
        let removed = remove_index_link(&index_content, &item.id);
        if removed == index_content {
            return Err(ObsidianPendingWorkStoreError::IndexLinkNotFound { id: item.id });
        }
        write_index(&index_path, &removed)?;
        std::fs::remove_file(item_path)
            .map_err(|source| ObsidianPendingWorkStoreError::RemoveItemFile { source })?;

        Ok(RemovedItem {
            id: item.id,
            project: item.project,
            title: item.session,
            deleted_path: item_path.to_path_buf(),
            unlinked: index_path.display().to_string(),
        })
    }
}
