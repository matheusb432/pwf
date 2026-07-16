use pwf_domain::pending_work::{OpenItem, ProjectRegistry, WorkItemId};

use crate::{AppDbStore, PendingWorkItem, pending_work::enrich::enrich};

#[derive(Debug, Clone)]
pub struct FindPendingWork {
    pub id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum FindPendingWorkError {
    /// No open item matched the requested id. Preserves the raw requested id
    /// verbatim (never re-normalized), matching the legacy lookup's display.
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    /// More than one open item — or more than one project sharing the id's
    /// prefix — matched. Preserves the raw requested id verbatim.
    #[error("Pending-work id is ambiguous: {id}")]
    AmbiguousId { id: String },
    /// The id's prefix maps to no managed project. Rendered exactly like the
    /// legacy adapter's `UnknownTaskPrefix` (uppercased id + prefix).
    #[error("Unknown task id prefix `{prefix}` for {id}")]
    UnknownPrefix { id: String, prefix: String },
    #[error("{0}")]
    ReadStore(Box<dyn std::error::Error + Send + Sync>),
}

/// Finds the single open pending-work item matching `id`, enriched with its
/// launch diagnostics — the shared lookup behind `pwf verify`, `pwf session`
/// dispatch validation, and prereq checks. Reuses [`enrich`] so those callers
/// see identical [`OpenItem`] data to the list view.
///
/// A canonical id routes to its project by prefix, listing that project's open
/// entries and matching by exact id; a non-canonical id is served by the
/// legacy inline scan (`<project>:<ordinal>` prompts have no [`WorkItemId`], so
/// they are found by listing every project and matching the composed id
/// case-insensitively). Item identity is index-open-link driven (not
/// note-status driven), exactly as the legacy open-item read was.
pub(crate) fn find_open_item<S>(
    store: &S,
    projects: &ProjectRegistry,
    id: &str,
) -> Result<OpenItem, FindPendingWorkError>
where
    S: AppDbStore<PendingWorkItem>,
{
    let requested = id.to_string();
    let Ok(work_id) = WorkItemId::try_new(id) else {
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
    let mut matched: Vec<OpenItem> = records
        .iter()
        .map(|record| enrich(record, repo.as_deref()).into_open_item(project.as_ref().to_string()))
        .filter(|item| item.id == work_id.as_ref())
        .collect();
    match matched.len() {
        0 => Err(FindPendingWorkError::ItemNotFound { id: requested }),
        1 => Ok(matched.pop().expect("length checked")),
        _ => Err(FindPendingWorkError::AmbiguousId { id: requested }),
    }
}

/// Scans every managed project's open entries for the legacy inline record whose
/// composed `<project>:<ordinal>` display id matches `id` case-insensitively.
fn find_inline_open_item<S>(
    store: &S,
    projects: &ProjectRegistry,
    requested: &str,
) -> Result<OpenItem, FindPendingWorkError>
where
    S: AppDbStore<PendingWorkItem>,
{
    for (project, repo) in projects.projects() {
        let records = store
            .list(project)
            .map_err(|error| FindPendingWorkError::ReadStore(Box::new(error)))?;
        for record in records {
            let item = enrich(&record, repo).into_open_item(project.as_ref().to_string());
            if item.format == "legacy" && item.id.eq_ignore_ascii_case(requested) {
                return Ok(item);
            }
        }
    }
    Err(FindPendingWorkError::ItemNotFound {
        id: requested.to_string(),
    })
}

#[cqrsy::handler(query)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "the cqrsy find operation owns its request by contract"
)]
pub fn execute(
    query: FindPendingWork,
    store: &impl AppDbStore<PendingWorkItem>,
    projects: &ProjectRegistry,
) -> Result<OpenItem, FindPendingWorkError> {
    find_open_item(store, projects, &query.id)
}

#[cfg(test)]
mod tests {
    use pwf_domain::pending_work::{
        ProjectName, ProjectRegistry, Timestamp, WorkItemId, WorkItemStatus,
    };

    use super::{FindPendingWork, FindPendingWorkError, execute};
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
            placement: None,
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
    ) -> Result<pwf_domain::pending_work::OpenItem, FindPendingWorkError> {
        execute(FindPendingWork { id: id.to_string() }, store, projects)
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
    fn canonical_id_is_case_insensitive_via_workitemid() {
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
