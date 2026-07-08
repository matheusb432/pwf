use std::path::Path;

use pwf_application::{PendingWorkResolveStore, ResolvePendingWorkOutput};

use super::{
    ObsidianPendingWorkStore, ObsidianPendingWorkStoreError,
    fs::{path_str, read_item_file},
};

const SHOW_FRONTMATTER_DENYLIST: &[&str] = &["created"];

impl PendingWorkResolveStore for ObsidianPendingWorkStore {
    type Error = ObsidianPendingWorkStoreError;

    fn resolve_item(&self, id: &str, show: bool) -> Result<ResolvePendingWorkOutput, Self::Error> {
        match self.find_pending_item(id) {
            Ok(item) => Self::resolve_open_item(&item, show),
            Err(ObsidianPendingWorkStoreError::ItemNotFound { id }) => {
                match self.find_item_note_file(&id) {
                    Some(path) => Self::resolve_note_file(&path, show),
                    None => Err(ObsidianPendingWorkStoreError::ItemNotFound { id }),
                }
            }
            Err(other) => Err(other),
        }
    }
}

impl ObsidianPendingWorkStore {
    pub(super) fn resolve_open_item(
        item: &pwf_domain::pending_work::OpenItem,
        show: bool,
    ) -> Result<ResolvePendingWorkOutput, ObsidianPendingWorkStoreError> {
        if show {
            return match item.item_file.as_deref() {
                Some(file) => Self::resolve_note_file(Path::new(file), true),
                None => Ok(ResolvePendingWorkOutput::NoteMarkdown(item.prompt.clone())),
            };
        }

        Ok(ResolvePendingWorkOutput::NotePath(
            item.item_file.as_deref().unwrap_or(&item.note).to_string(),
        ))
    }

    pub(super) fn resolve_note_file(
        file: &Path,
        show: bool,
    ) -> Result<ResolvePendingWorkOutput, ObsidianPendingWorkStoreError> {
        if show {
            let raw = read_item_file(file)?;
            return Ok(ResolvePendingWorkOutput::NoteMarkdown(
                pwf_core::frontmatter::strip_frontmatter_keys(&raw, SHOW_FRONTMATTER_DENYLIST),
            ));
        }

        Ok(ResolvePendingWorkOutput::NotePath(path_str(file)))
    }
}
