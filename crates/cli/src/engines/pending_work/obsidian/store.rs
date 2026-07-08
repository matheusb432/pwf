use std::path::Path;

pub(in crate::engines::pending_work) struct ObsidianStore;

impl ObsidianStore {
    pub(in crate::engines::pending_work) fn read_text_or_default(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    pub(in crate::engines::pending_work) fn read_text_optional(path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }
}
