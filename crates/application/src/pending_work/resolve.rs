use pwf_domain::pending_work::WorkItemId;

use super::{
    enrich::inline_record_id, project_registry::ProjectRegistry, show::ShowPendingWorkError,
};
use crate::{AppDbStore, PendingWorkItem, RecordId};

/// Resolves an open or closed record by project prefix or inline `<project>:<ordinal>` id.
pub(crate) fn resolve_record<S>(
    store: &S,
    projects: &ProjectRegistry,
    id: &str,
) -> Result<PendingWorkItem, ShowPendingWorkError>
where
    S: AppDbStore<PendingWorkItem>,
{
    let not_found = || ShowPendingWorkError::ItemNotFound { id: id.to_string() };
    let Ok(work_id) = WorkItemId::try_new(id) else {
        return resolve_inline_record(store, projects, id);
    };
    let project = projects.project_for_id(&work_id).ok_or_else(not_found)?;
    store
        .get(project, &work_id)
        .map_err(|error| ShowPendingWorkError::ReadStore(Box::new(error)))?
        .ok_or_else(not_found)
}

/// Finds an inline `<project>:<ordinal>` id case-insensitively across managed projects.
fn resolve_inline_record<S>(
    store: &S,
    projects: &ProjectRegistry,
    id: &str,
) -> Result<PendingWorkItem, ShowPendingWorkError>
where
    S: AppDbStore<PendingWorkItem>,
{
    for (project, _repo) in projects.projects() {
        let records = store
            .list(project)
            .map_err(|error| ShowPendingWorkError::ReadStore(Box::new(error)))?;
        let found = records.into_iter().find(|record| match record.id {
            RecordId::Inline(ordinal) => {
                inline_record_id(project.as_ref(), ordinal).eq_ignore_ascii_case(id)
            }
            RecordId::Item(_) => false,
        });
        if let Some(record) = found {
            return Ok(record);
        }
    }
    Err(ShowPendingWorkError::ItemNotFound { id: id.to_string() })
}

#[cfg(test)]
pub(crate) mod testing {
    use pwf_domain::pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus};

    use super::ProjectRegistry;
    use crate::{Materialization, PendingWorkItem, RecordId, testing::InMemoryStore};

    pub(crate) const PWF_0001_SOURCE: &str = "---\nid: PWF-0001\nstatus: active\ntitle: do the thing\nproject: pwf\ncreated: 2026-06-20\n---\n\n## Goals\n- do the thing\n";

    pub(crate) fn staged() -> (InMemoryStore, ProjectRegistry) {
        let record = PendingWorkItem {
            id: RecordId::Item(WorkItemId::try_new("PWF-0001").unwrap()),
            title: "do the thing".to_string(),
            status: WorkItemStatus::Active,
            created: Some(Timestamp::new("2026-06-20")),
            completed: None,
            commits: None,
            tags: None,
            effort: None,
            prereq: None,
            section: None,
            body: "\n## Goals\n- do the thing\n".to_string(),
            source: PWF_0001_SOURCE.to_string(),
            locator: "/notes/pwf/PWF-0001.md".to_string(),
            placement: None,
            materialization: Materialization::NoteFile,
        };
        let store = InMemoryStore::default().with_project("pwf", vec![record]);
        (store, registry())
    }

    pub(crate) fn staged_ghost() -> (InMemoryStore, ProjectRegistry) {
        let record = PendingWorkItem {
            id: RecordId::Item(WorkItemId::try_new("PWF-0002").unwrap()),
            title: "ghost".to_string(),
            status: WorkItemStatus::Active,
            created: None,
            completed: None,
            commits: None,
            tags: None,
            effort: None,
            prereq: None,
            section: None,
            body: String::new(),
            source: String::new(),
            locator: "/notes/pwf/PWF-0002.md".to_string(),
            placement: None,
            materialization: Materialization::MissingNote {
                expected: "/notes/pwf/PWF-0002.md".to_string(),
            },
        };
        let store = InMemoryStore::default().with_project("pwf", vec![record]);
        (store, registry())
    }

    pub(crate) fn staged_inline() -> (InMemoryStore, ProjectRegistry) {
        let record = PendingWorkItem {
            id: RecordId::Inline(1),
            title: "legacy task".to_string(),
            status: WorkItemStatus::Active,
            created: None,
            completed: None,
            commits: None,
            tags: None,
            effort: None,
            prereq: None,
            section: None,
            body: "do the legacy thing".to_string(),
            source: "do the legacy thing".to_string(),
            locator: "/notes/pwf/pwf.md".to_string(),
            placement: None,
            materialization: Materialization::InlineLegacy,
        };
        let store = InMemoryStore::default().with_project("pwf", vec![record]);
        (store, registry())
    }

    fn registry() -> ProjectRegistry {
        ProjectRegistry::new(vec![(
            ProjectName::try_new("pwf").unwrap(),
            Some("/repo/pwf".to_string()),
            Some("PWF".to_string()),
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ShowPendingWorkError, resolve_record,
        testing::{staged, staged_ghost, staged_inline},
    };

    #[test]
    fn resolve_record_returns_path_and_markdown() {
        let (store, registry) = staged();

        let resolved = resolve_record(&store, &registry, "PWF-0001").unwrap();

        assert_eq!(resolved.locator, "/notes/pwf/PWF-0001.md");
        assert_eq!(resolved.source, super::testing::PWF_0001_SOURCE);
    }

    #[test]
    fn resolve_returns_expected_note_path_for_missing_note_wikilink() {
        let (store, registry) = staged_ghost();

        let resolved = resolve_record(&store, &registry, "PWF-0002").unwrap();

        assert_eq!(resolved.locator, "/notes/pwf/PWF-0002.md");
    }

    #[test]
    fn resolve_serves_inline_legacy_id_case_insensitively() {
        let (store, registry) = staged_inline();

        let resolved = resolve_record(&store, &registry, "PWF:1").unwrap();

        assert_eq!(resolved.locator, "/notes/pwf/pwf.md");
        assert_eq!(resolved.source, "do the legacy thing");
    }

    #[test]
    fn resolve_unknown_inline_id_preserves_raw_id() {
        let (store, registry) = staged_inline();

        let error = resolve_record(&store, &registry, "pwf:9").unwrap_err();

        assert_eq!(error.to_string(), "Open pending-work item not found: pwf:9");
    }

    #[test]
    fn resolve_missing_id_preserves_raw_lowercase_id() {
        let (store, registry) = staged();

        let error = resolve_record(&store, &registry, "pwf-9999").unwrap_err();

        assert_eq!(
            error.to_string(),
            "Open pending-work item not found: pwf-9999"
        );
        assert!(matches!(
            error,
            ShowPendingWorkError::ItemNotFound { id } if id == "pwf-9999"
        ));
    }
}
