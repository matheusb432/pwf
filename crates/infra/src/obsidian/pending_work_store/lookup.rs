use std::path::PathBuf;

use pwf_application::PendingWorkReadStore;
use pwf_domain::pending_work::{OpenItem, ProjectName, WorkItemId, WorkItemStatus};

use super::{ObsidianPendingWorkStore, ObsidianPendingWorkStoreError};
use crate::obsidian::identity::inspect_project_task_notes;

pub(super) struct TaskFile {
    pub(super) id: WorkItemId,
    pub(super) path: PathBuf,
    pub(super) markdown: String,
    pub(super) title: Option<String>,
}

impl ObsidianPendingWorkStore {
    pub(super) fn find_pending_item(
        &self,
        id: &str,
    ) -> Result<OpenItem, ObsidianPendingWorkStoreError> {
        let requested = id.to_string();
        let Ok(id) = WorkItemId::try_new(id) else {
            return self.find_legacy_pending_item(&requested);
        };
        let prefix = id.as_ref().split_once('-').map_or("", |(prefix, _)| prefix);
        if self
            .config
            .prefixes
            .values()
            .filter(|candidate| candidate.eq_ignore_ascii_case(prefix))
            .take(2)
            .count()
            > 1
        {
            return Err(ObsidianPendingWorkStoreError::AmbiguousId { id: requested });
        }
        let project = self.project_for_id(&id)?;
        let items = self.open_items_for_project(&project)?;
        let selected: Vec<&OpenItem> = items.iter().filter(|item| item.id == id.as_ref()).collect();
        match selected.as_slice() {
            [] => Err(ObsidianPendingWorkStoreError::ItemNotFound { id: requested }),
            [item] => Ok((*item).clone()),
            _ => Err(ObsidianPendingWorkStoreError::AmbiguousId { id: requested }),
        }
    }

    fn find_legacy_pending_item(
        &self,
        requested: &str,
    ) -> Result<OpenItem, ObsidianPendingWorkStoreError> {
        let items = self.all_open_items()?;
        items
            .into_iter()
            .find(|item| item.format == "legacy" && item.id.eq_ignore_ascii_case(requested))
            .ok_or_else(|| ObsidianPendingWorkStoreError::ItemNotFound {
                id: requested.to_string(),
            })
    }

    pub(super) fn task_files_for_project(
        &self,
        project: &ProjectName,
    ) -> Result<Vec<TaskFile>, ObsidianPendingWorkStoreError> {
        let project_dir = pwf_core::paths::project_dir(
            self.config.notes_dir_for(project.as_ref()),
            project.as_ref(),
        );
        let index_path = pwf_core::paths::project_index_path(
            self.config.notes_dir_for(project.as_ref()),
            project.as_ref(),
        );
        if !project_dir.exists() {
            return Ok(Vec::new());
        }
        let prefix = self.config.prefixes.get(project.as_ref()).ok_or_else(|| {
            ObsidianPendingWorkStoreError::ProjectMissingPrefix {
                project: project.as_ref().to_string(),
            }
        })?;
        inspect_project_task_notes(&project_dir, &index_path, prefix, project.as_ref()).map(
            |tasks| {
                tasks
                    .into_iter()
                    .map(|task| TaskFile {
                        id: task.id,
                        path: task.path,
                        markdown: task.markdown,
                        title: task.title,
                    })
                    .collect()
            },
        )
    }

    pub(super) fn next_task_id(
        &self,
        project: &ProjectName,
        prefix: &str,
    ) -> Result<String, ObsidianPendingWorkStoreError> {
        let maximum = self
            .task_files_for_project(project)?
            .into_iter()
            .filter_map(|task| {
                task.id
                    .as_ref()
                    .split_once('-')
                    .and_then(|(_, number)| number.parse::<u32>().ok())
            })
            .max()
            .unwrap_or(0);
        Ok(format!("{prefix}-{:04}", maximum + 1))
    }

    fn project_for_id(
        &self,
        id: &WorkItemId,
    ) -> Result<ProjectName, ObsidianPendingWorkStoreError> {
        let prefix = id.as_ref().split_once('-').map_or("", |(prefix, _)| prefix);
        let project = self
            .config
            .prefixes
            .iter()
            .find(|(_, candidate)| candidate.eq_ignore_ascii_case(prefix))
            .map(|(project, _)| project)
            .ok_or_else(|| ObsidianPendingWorkStoreError::UnknownTaskPrefix {
                id: id.as_ref().to_string(),
                prefix: prefix.to_string(),
            })?;
        Ok(ProjectName::try_new(project).expect("configured project is non-empty"))
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

    pub(super) fn find_item_note_file(
        &self,
        id: &str,
    ) -> Result<Option<PathBuf>, ObsidianPendingWorkStoreError> {
        let id = WorkItemId::try_new(id)
            .map_err(|_| ObsidianPendingWorkStoreError::ItemNotFound { id: id.to_string() })?;
        let project = self.project_for_id(&id)?;
        Ok(self
            .task_files_for_project(&project)?
            .into_iter()
            .find(|task| task.id == id)
            .map(|task| task.path))
    }

    pub(super) fn find_item_note_with_project(
        &self,
        id: &str,
    ) -> Result<Option<(String, PathBuf)>, ObsidianPendingWorkStoreError> {
        let id = WorkItemId::try_new(id)
            .map_err(|_| ObsidianPendingWorkStoreError::ItemNotFound { id: id.to_string() })?;
        let project = self.project_for_id(&id)?;
        Ok(self
            .task_files_for_project(&project)?
            .into_iter()
            .find(|task| task.id == id)
            .map(|task| (project.as_ref().to_string(), task.path)))
    }

    /// Public lookup for cross-engine callers (the handoff mirror gate):
    /// the owning project and note path for `id`, open or closed.
    pub fn note_with_project(
        &self,
        id: &str,
    ) -> Result<Option<(String, PathBuf)>, ObsidianPendingWorkStoreError> {
        self.find_item_note_with_project(id)
    }
}

pub(super) fn normalize_lookup_id(id: &str) -> String {
    WorkItemId::try_new(id).map_or_else(|_| id.to_string(), |id| id.as_ref().to_string())
}
