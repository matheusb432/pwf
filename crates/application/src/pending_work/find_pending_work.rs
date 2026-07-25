use crate::{
    AppRecordStore, PendingWorkItem,
    pending_work::{
        enrich::{enrich, is_open_item},
        get_pending_work::PendingWorkItemView,
        identifier,
        project_registry::ProjectRegistry,
    },
};

#[derive(Debug, Clone)]
pub struct FindPendingWork {
    pub id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum FindPendingWorkError {
    /// Preserves the unmatched requested id without normalizing it again.
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    /// Reports multiple matching items or projects sharing one id prefix.
    #[error("Pending-work id is ambiguous: {id}")]
    AmbiguousId { id: String },
    #[error("Unknown task id prefix `{prefix}` for {id}")]
    UnknownPrefix { id: String, prefix: String },
    #[error("{0}")]
    ReadStore(Box<dyn std::error::Error + Send + Sync>),
}

/// Finds one open item and applies the same launchability enrichment as list.
///
/// Canonical ids route by project prefix. Inline `<project>:<ordinal>` ids require a
/// case-insensitive scan across managed projects. Open index links determine membership.
pub(crate) fn find_open_item<S>(
    store: &S,
    projects: &ProjectRegistry,
    id: &str,
) -> Result<PendingWorkItemView, FindPendingWorkError>
where
    S: AppRecordStore<PendingWorkItem>,
{
    let requested = id.to_string();
    let Some(work_id) = identifier::parse(id) else {
        return find_inline_open_item(store, projects, &requested);
    };
    let prefix = work_id
        .as_ref()
        .split_once('-')
        .map_or("", |(prefix, _)| prefix);
    if projects.projects_with_prefix(prefix) > 1 {
        return Err(FindPendingWorkError::AmbiguousId { id: requested });
    }
    let project =
        projects
            .project_for_id(&work_id)
            .ok_or_else(|| FindPendingWorkError::UnknownPrefix {
                id: work_id.as_ref().to_string(),
                prefix: prefix.to_string(),
            })?;
    let repo = projects.repo_for(project).map(str::to_string);
    let records = store
        .list(project)
        .map_err(|error| FindPendingWorkError::ReadStore(Box::new(error)))?;
    let mut matched: Vec<PendingWorkItemView> = records
        .iter()
        .filter(|record| is_open_item(record))
        .map(|record| {
            enrich(record, repo.as_deref())
                .into_pending_work_item_view(project.as_ref().to_string())
        })
        .filter(|item| item.id == work_id.as_ref())
        .collect();
    match matched.len() {
        0 => Err(FindPendingWorkError::ItemNotFound { id: requested }),
        1 => Ok(matched.pop().expect("length checked")),
        _ => Err(FindPendingWorkError::AmbiguousId { id: requested }),
    }
}

/// Finds an inline `<project>:<ordinal>` id case-insensitively across managed projects.
fn find_inline_open_item<S>(
    store: &S,
    projects: &ProjectRegistry,
    requested: &str,
) -> Result<PendingWorkItemView, FindPendingWorkError>
where
    S: AppRecordStore<PendingWorkItem>,
{
    for (project, repo) in projects.projects() {
        let records = store
            .list(project)
            .map_err(|error| FindPendingWorkError::ReadStore(Box::new(error)))?;
        for record in records {
            if !is_open_item(&record) {
                continue;
            }
            let item =
                enrich(&record, repo).into_pending_work_item_view(project.as_ref().to_string());
            if item.format == "legacy" && item.id.eq_ignore_ascii_case(requested) {
                return Ok(item);
            }
        }
    }
    Err(FindPendingWorkError::ItemNotFound {
        id: requested.to_string(),
    })
}

#[cqrsy::query]
pub fn execute(
    query: &FindPendingWork,
    store: &impl AppRecordStore<PendingWorkItem>,
    projects: &ProjectRegistry,
) -> Result<PendingWorkItemView, FindPendingWorkError> {
    find_open_item(store, projects, &query.id)
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use pwf_domain::pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus};

    use super::{FindPendingWork, FindPendingWorkError, PendingWorkItemView, ProjectRegistry};
    use crate::{
        IndexPlacement, Materialization, PendingWorkItem, RecordId, testing::InMemoryStore,
    };

    fn record(id: &str) -> PendingWorkItem {
        PendingWorkItem {
            id: RecordId::Item(WorkItemId::try_new(id).unwrap()),
            title: format!("title {id}"),
            status: WorkItemStatus::Active,
            created: Some(Timestamp::new("2026-07-07")),
            completed: None,
            commits: None,
            tags: None,
            effort: None,
            prereq: None,
            section: None,
            body: "do the thing".to_string(),
            source: String::new(),
            locator: format!("/notes/pwf/{id}.md"),
            placement: Some(IndexPlacement {
                index_path: "/notes/pwf/pwf.md".to_string(),
                line: 7,
            }),
            materialization: Materialization::NoteFile,
        }
    }

    fn inline(ordinal: usize) -> PendingWorkItem {
        PendingWorkItem {
            id: RecordId::Inline(ordinal),
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
            placement: Some(IndexPlacement {
                index_path: "/notes/pwf/pwf.md".to_string(),
                line: ordinal,
            }),
            materialization: Materialization::InlineLegacy,
        }
    }

    fn registry(projects: &[(&str, &str)]) -> ProjectRegistry {
        ProjectRegistry::new(projects.iter().map(|(name, prefix)| {
            (
                ProjectName::try_new(*name).unwrap(),
                Some(format!("/repo/{name}")),
                Some((*prefix).to_string()),
            )
        }))
    }

    fn find(
        store: &InMemoryStore,
        projects: &ProjectRegistry,
        id: &str,
    ) -> Result<PendingWorkItemView, FindPendingWorkError> {
        super::execute(&FindPendingWork { id: id.to_string() }, store, projects)
    }

    #[test]
    fn finds_open_item_enriched_with_launchability() {
        let store = InMemoryStore::default().with_project("pwf", vec![record("PWF-0001")]);
        let projects = registry(&[("pwf", "PWF")]);

        let item = find(&store, &projects, "PWF-0001").unwrap();

        assert_eq!(item.id, "PWF-0001");
        assert_eq!(item.project, "pwf");
        assert_eq!(item.repo.as_deref(), Some("/repo/pwf"));
        assert_eq!(item.prompt, "do the thing");
        assert!(item.launchable);
    }

    #[test]
    fn loose_id_normalization_is_application_owned() {
        let store = InMemoryStore::default().with_project("pwf", vec![record("PWF-0001")]);
        let projects = registry(&[("pwf", "PWF")]);

        assert_eq!(find(&store, &projects, "pwf-0001").unwrap().id, "PWF-0001");
    }

    #[test]
    fn missing_id_errors_not_found_preserving_raw_id() {
        let store = InMemoryStore::default().with_project("pwf", vec![record("PWF-0001")]);
        let projects = registry(&[("pwf", "PWF")]);

        let error = find(&store, &projects, "pwf-9999").unwrap_err();

        assert!(matches!(
            error,
            FindPendingWorkError::ItemNotFound { ref id } if id == "pwf-9999"
        ));
        assert_eq!(
            error.to_string(),
            "Open pending-work item not found: pwf-9999"
        );
    }

    #[test]
    fn shared_prefix_across_projects_is_ambiguous_preserving_raw_id() {
        let store = InMemoryStore::default()
            .with_project("alpha", vec![record("PWF-0001")])
            .with_project("beta", vec![record("PWF-0001")]);
        let projects = registry(&[("alpha", "PWF"), ("beta", "PWF")]);

        let error = find(&store, &projects, "pwf-0001").unwrap_err();

        assert!(matches!(
            error,
            FindPendingWorkError::AmbiguousId { ref id } if id == "pwf-0001"
        ));
        assert_eq!(error.to_string(), "Pending-work id is ambiguous: pwf-0001");
    }

    #[test]
    fn duplicate_link_in_one_project_is_ambiguous() {
        let store = InMemoryStore::default()
            .with_project("pwf", vec![record("PWF-0001"), record("PWF-0001")]);
        let projects = registry(&[("pwf", "PWF")]);

        let error = find(&store, &projects, "PWF-0001").unwrap_err();

        assert!(matches!(error, FindPendingWorkError::AmbiguousId { .. }));
    }

    #[test]
    fn find_rejects_closed_and_unlinked_active_records() {
        let done = PendingWorkItem {
            status: WorkItemStatus::Done,
            placement: None,
            ..record("PWF-0001")
        };
        let unlinked_active = PendingWorkItem {
            placement: None,
            ..record("PWF-0002")
        };
        let store = InMemoryStore::default().with_project("pwf", vec![done, unlinked_active]);
        let projects = registry(&[("pwf", "PWF")]);

        for id in ["PWF-0001", "PWF-0002"] {
            assert_matches!(
                find(&store, &projects, id),
                Err(FindPendingWorkError::ItemNotFound { id: missing }) if missing == id
            );
        }
    }

    #[test]
    fn unknown_prefix_renders_like_the_legacy_adapter() {
        let store = InMemoryStore::default().with_project("pwf", vec![record("PWF-0001")]);
        let projects = registry(&[("pwf", "PWF")]);

        let error = find(&store, &projects, "xyz-0001").unwrap_err();

        assert!(matches!(
            error,
            FindPendingWorkError::UnknownPrefix { ref id, ref prefix }
                if id == "XYZ-0001" && prefix == "XYZ"
        ));
        assert_eq!(
            error.to_string(),
            "Unknown task id prefix `XYZ` for XYZ-0001"
        );
    }

    #[test]
    fn non_canonical_id_resolves_a_legacy_inline_prompt_case_insensitively() {
        let store = InMemoryStore::default().with_project("pwf", vec![inline(1)]);
        let projects = registry(&[("pwf", "PWF")]);

        let item = find(&store, &projects, "PWF:1").unwrap();

        assert_eq!(item.id, "pwf:1");
        assert_eq!(item.format, "legacy");
        assert_eq!(item.session, "legacy task");
    }
}
