use std::path::{Path, PathBuf};

use pwf_application::PendingWorkReadStore;
use pwf_domain::pending_work::{OpenItem, WorkItemId, WorkItemStatus};

use super::{ObsidianPendingWorkStore, ObsidianPendingWorkStoreError, fs::project_archive_dir};

impl ObsidianPendingWorkStore {
    pub(super) fn find_pending_item(
        &self,
        id: &str,
    ) -> Result<OpenItem, ObsidianPendingWorkStoreError> {
        let requested = id.to_string();
        let normalized = normalize_lookup_id(id);
        let items = self.open_items(None)?;
        let selected: Vec<&OpenItem> = items
            .iter()
            .filter(|item| item.id.eq_ignore_ascii_case(&normalized))
            .collect();
        match selected.as_slice() {
            [] => Err(ObsidianPendingWorkStoreError::ItemNotFound { id: requested }),
            [item] => Ok((*item).clone()),
            _ => Err(ObsidianPendingWorkStoreError::AmbiguousId { id: requested }),
        }
    }

    pub(super) fn read_status(&self, id: &str) -> Option<WorkItemStatus> {
        let prefix = id.split('-').next().unwrap_or("");
        let project = self
            .config
            .prefixes
            .iter()
            .find(|(_, candidate)| candidate.as_str() == prefix)
            .map(|(project, _)| project.as_str())?;
        let path = pwf_core::paths::project_dir(self.config.notes_dir_for(project), project)
            .join(format!("{id}.md"));
        let raw = std::fs::read_to_string(path).ok()?;
        pwf_core::frontmatter::parse(&raw)
            .frontmatter
            .get("status")
            .and_then(|status| status.parse().ok())
    }

    pub(super) fn find_item_note_file(&self, id: &str) -> Option<PathBuf> {
        for project in self.config.projects.keys() {
            let base = self.config.notes_dir_for(project);
            for dir in [
                pwf_core::paths::project_dir(base, project),
                project_archive_dir(base, project),
            ] {
                if let Some(path) = scan_dir_for_item_note(&dir, id) {
                    return Some(path);
                }
            }
        }
        None
    }

    pub(super) fn find_item_note_with_project(&self, id: &str) -> Option<(String, PathBuf)> {
        let requested = normalize_lookup_id(id);
        for project in self.config.projects.keys() {
            let base = self.config.notes_dir_for(project);
            for dir in [
                pwf_core::paths::project_dir(base, project),
                project_archive_dir(base, project),
            ] {
                if let Some(path) = scan_dir_for_item_note(&dir, &requested) {
                    return Some((project.clone(), path));
                }
            }
        }
        None
    }

    /// Public lookup for cross-engine callers (the handoff mirror gate):
    /// the owning project and note path for `id`, open or closed.
    pub fn note_with_project(&self, id: &str) -> Option<(String, PathBuf)> {
        self.find_item_note_with_project(id)
    }
}

pub(super) fn normalize_lookup_id(id: &str) -> String {
    WorkItemId::try_new(id).map_or_else(|_| id.to_string(), |id| id.as_ref().to_string())
}

fn scan_dir_for_item_note(dir: &Path, id: &str) -> Option<PathBuf> {
    let requested = normalize_lookup_id(id);
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
            continue;
        }
        if path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| normalize_lookup_id(stem) == requested)
        {
            return Some(path);
        }
    }
    None
}
