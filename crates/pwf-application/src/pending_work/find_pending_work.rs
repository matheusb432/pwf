use crate::{
    pending_work::{dto::PendingWorkItemView, logic::finding::find_open_item_from_db},
    ports::pending_work_record::PendingWorkStore,
};

#[derive(Debug, Clone)]
pub struct FindPendingWork {
    pub id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum FindPendingWorkError {
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    #[error("Pending-work id is ambiguous: {id}")]
    AmbiguousId { id: String },
    #[error("Unknown task id prefix `{prefix}` for {id}")]
    UnknownPrefix { id: String, prefix: String },
    #[error("{0}")]
    ReadStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    QueryProject(Box<dyn std::error::Error + Send + Sync>),
}

#[cqrsy::query]
pub async fn execute(
    query: &FindPendingWork,
    store: &impl PendingWorkStore,
    pool: &sqlx::SqlitePool,
) -> Result<PendingWorkItemView, FindPendingWorkError> {
    find_open_item_from_db(store, pool, &query.id).await
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use pwf_models::{
        pending_work::{Timestamp, WorkItemId, WorkItemStatus},
        project::Project,
    };

    use super::{FindPendingWorkError, PendingWorkItemView};
    use crate::{
        pending_work::logic::finding::find_open_item_in_projects,
        ports::pending_work_record::{
            IndexPlacement, Materialization, PendingWorkRecord, RecordId,
        },
        testing::{InMemoryStore, project},
    };

    fn record(id: &str) -> PendingWorkRecord {
        PendingWorkRecord {
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

    fn inline(ordinal: usize) -> PendingWorkRecord {
        PendingWorkRecord {
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

    fn projects(values: &[(&str, &str)]) -> Vec<Project> {
        values
            .iter()
            .map(|(name, project_id)| project(project_id, name))
            .collect()
    }

    fn find(
        store: &InMemoryStore,
        projects: &[Project],
        id: &str,
    ) -> Result<PendingWorkItemView, FindPendingWorkError> {
        find_open_item_in_projects(store, projects, id)
    }

    #[test]
    fn finds_open_item_enriched_with_launchability() {
        let store = InMemoryStore::default().with_project("pwf", vec![record("PWF-0001")]);
        let projects = projects(&[("pwf", "PWF")]);

        let item = find(&store, &projects, "PWF-0001").unwrap();

        assert_eq!(item.id, "PWF-0001");
        assert_eq!(item.project, "pwf");
        assert_eq!(item.repo.as_deref(), Some("/work/pwf"));
        assert_eq!(item.prompt, "do the thing");
        assert!(item.launchable);
    }

    #[test]
    fn loose_id_normalization_is_application_owned() {
        let store = InMemoryStore::default().with_project("pwf", vec![record("PWF-0001")]);
        let projects = projects(&[("pwf", "PWF")]);

        assert_eq!(find(&store, &projects, "pwf-0001").unwrap().id, "PWF-0001");
    }

    #[test]
    fn missing_id_errors_not_found_preserving_raw_id() {
        let store = InMemoryStore::default().with_project("pwf", vec![record("PWF-0001")]);
        let projects = projects(&[("pwf", "PWF")]);

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
    fn duplicate_link_in_one_project_is_ambiguous() {
        let store = InMemoryStore::default()
            .with_project("pwf", vec![record("PWF-0001"), record("PWF-0001")]);
        let projects = projects(&[("pwf", "PWF")]);

        let error = find(&store, &projects, "PWF-0001").unwrap_err();

        assert!(matches!(error, FindPendingWorkError::AmbiguousId { .. }));
    }

    #[test]
    fn find_rejects_closed_and_unlinked_active_records() {
        let done = PendingWorkRecord {
            status: WorkItemStatus::Done,
            placement: None,
            ..record("PWF-0001")
        };
        let unlinked_active = PendingWorkRecord {
            placement: None,
            ..record("PWF-0002")
        };
        let store = InMemoryStore::default().with_project("pwf", vec![done, unlinked_active]);
        let projects = projects(&[("pwf", "PWF")]);

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
        let projects = projects(&[("pwf", "PWF")]);

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
        let projects = projects(&[("pwf", "PWF")]);

        let item = find(&store, &projects, "PWF:1").unwrap();

        assert_eq!(item.id, "pwf:1");
        assert_eq!(item.format, "legacy");
        assert_eq!(item.session, "legacy task");
    }
}
