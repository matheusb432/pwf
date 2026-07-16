use std::path::PathBuf;

use pwf_domain::pending_work::{AddedItem, ProjectName, ProjectRegistry, Tags, Timestamp};

use super::store_util;
use crate::ports::{AppDbStore, IndexEntry, IndexSection, NewItem, PendingWorkItem};

#[derive(Debug, Clone)]
pub struct AddPendingWorkItem {
    pub project_name: String,
    pub prompt: String,
    pub title: Option<String>,
    pub created: String,
    pub section: Option<String>,
    pub prereq: Option<String>,
    pub effort: Option<u8>,
    pub tags: Option<Tags>,
}

#[derive(Debug, thiserror::Error)]
pub enum AddPendingWorkError {
    /// Verbatim former infra display (PWF-0123 error-string relocation).
    #[error("Project '{project}' is not mapped to a repo in config/pending-work.json.")]
    ProjectNotMappedToRepo { project: String },
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
}

/// Creates a pending-work item: validates the project's repo mapping via the
/// registry, then inserts the record + open index entry through
/// [`store_util::create_item`], returning the typed [`AddedItem`] outcome
/// (including the created-section fact behind the CLI's stderr diagnostic).
///
/// # Panics
///
/// Panics if the store's `insert` violates its contract by returning a record
/// without a canonical [`pwf_domain::pending_work::WorkItemId`].
#[cqrsy::handler(command)]
pub fn execute<S>(
    cmd: AddPendingWorkItem,
    store: &S,
    projects: &ProjectRegistry,
) -> Result<AddedItem, AddPendingWorkError>
where
    S: AppDbStore<PendingWorkItem> + AppDbStore<IndexEntry> + AppDbStore<IndexSection>,
{
    let not_mapped = |project: &str| AddPendingWorkError::ProjectNotMappedToRepo {
        project: project.to_string(),
    };
    let Ok(project) = ProjectName::try_new(&cmd.project_name) else {
        return Err(not_mapped(&cmd.project_name));
    };
    if projects
        .repo_for(&project)
        .is_none_or(|repo| repo.trim().is_empty())
    {
        return Err(not_mapped(&cmd.project_name));
    }

    let created = store_util::create_item(
        store,
        &project,
        NewItem {
            prompt: cmd.prompt,
            title: cmd.title,
            created: Timestamp::new(cmd.created),
            section: cmd.section,
            prereq: cmd.prereq,
            effort: cmd.effort,
            tags: cmd.tags,
        },
    )
    .map_err(AddPendingWorkError::WriteStore)?;

    let id = created
        .record
        .id
        .as_item()
        .expect("inserted record carries a canonical id")
        .as_ref()
        .to_string();
    Ok(AddedItem {
        id,
        project: cmd.project_name,
        title: created.record.title,
        note_path: PathBuf::from(created.record.locator),
        created_section: created.created_section,
    })
}

#[cfg(test)]
mod tests {
    use pwf_domain::pending_work::{ProjectName, ProjectRegistry};

    use super::{AddPendingWorkError, AddPendingWorkItem, execute};
    use crate::{IndexEntryState, testing::InMemoryStore};

    fn registry(repo: Option<&str>) -> ProjectRegistry {
        ProjectRegistry::new(vec![(
            ProjectName::try_new("pwf").unwrap(),
            repo.map(str::to_string),
            Some("PWF".to_string()),
        )])
    }

    fn command(section: Option<&str>) -> AddPendingWorkItem {
        AddPendingWorkItem {
            project_name: "pwf".to_string(),
            prompt: "do the thing".to_string(),
            title: Some("ship it".to_string()),
            created: "2026-07-15".to_string(),
            section: section.map(str::to_string),
            prereq: None,
            effort: None,
            tags: None,
        }
    }

    #[test]
    fn add_inserts_record_and_open_index_entry() {
        let store = InMemoryStore::default().with_prefix("pwf", "PWF");

        let added = execute(command(None), &store, &registry(Some("/repo/pwf"))).unwrap();

        assert_eq!(added.id, "PWF-0001");
        assert_eq!(added.project, "pwf");
        assert_eq!(added.title, "ship it");
        assert_eq!(added.created_section, None);
        let entries = store.entries("pwf");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id.as_ref(), "PWF-0001");
        assert_eq!(entries[0].state, IndexEntryState::Open);
        assert_eq!(store.items("pwf").len(), 1);
    }

    #[test]
    fn add_reports_created_section_only_when_region_absent() {
        let store = InMemoryStore::default().with_prefix("pwf", "PWF");

        let added = execute(command(Some("Human")), &store, &registry(Some("/repo/pwf"))).unwrap();

        assert_eq!(added.created_section.as_deref(), Some("Human"));
    }

    /// A `## Human` header with zero entries already exists — the diagnostic
    /// must NOT fire (the legacy `section_exists` contract).
    #[test]
    fn add_does_not_report_created_section_for_existing_empty_region() {
        let store = InMemoryStore::default()
            .with_prefix("pwf", "PWF")
            .with_sections("pwf", &["Human"]);

        let added = execute(command(Some("Human")), &store, &registry(Some("/repo/pwf"))).unwrap();

        assert_eq!(added.created_section, None);
    }

    #[test]
    fn add_rejects_unmapped_project_with_legacy_display() {
        let store = InMemoryStore::default().with_prefix("pwf", "PWF");

        for registry in [registry(None), registry(Some("  "))] {
            let error = execute(command(None), &store, &registry).unwrap_err();
            assert!(matches!(
                error,
                AddPendingWorkError::ProjectNotMappedToRepo { ref project } if project == "pwf"
            ));
            assert_eq!(
                error.to_string(),
                "Project 'pwf' is not mapped to a repo in config/pending-work.json."
            );
        }
    }
}
