use pwf_domain::pending_work::{ProjectRegistry, WorkItemId};

use crate::{AppDbStore, PendingWorkItem};

#[derive(Debug, Clone)]
pub struct QueryItemTags {
    pub id: String,
}

/// Contains the item identity and raw tags needed by the handoff mirror gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemTags {
    pub project: String,
    pub id: String,
    /// Preserves raw `tags:` frontmatter for validation by the caller.
    pub tags: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum QueryItemTagsError {
    /// Rejects non-canonical ids instead of treating them as a missing item.
    #[error("Open pending-work item not found: {id}")]
    NotCanonical { id: String },
    #[error("Unknown task id prefix `{prefix}` for {id}")]
    UnknownPrefix { id: String, prefix: String },
    #[error("{0}")]
    ReadStore(Box<dyn std::error::Error + Send + Sync>),
}

/// Reads raw tags for an open or closed item routed by canonical id prefix.
///
/// `Ok(None)` means the id and prefix are valid but no record exists. Invalid ids and unmanaged
/// prefixes are errors.
#[cqrsy::query]
pub fn execute(
    query: &QueryItemTags,
    store: &impl AppDbStore<PendingWorkItem>,
    projects: &ProjectRegistry,
) -> Result<Option<ItemTags>, QueryItemTagsError> {
    let work_id = WorkItemId::try_new(&query.id).map_err(|_| QueryItemTagsError::NotCanonical {
        id: query.id.clone(),
    })?;
    let prefix = work_id
        .as_ref()
        .split_once('-')
        .map_or("", |(prefix, _)| prefix);
    let project =
        projects
            .project_for_id(&work_id)
            .ok_or_else(|| QueryItemTagsError::UnknownPrefix {
                id: work_id.as_ref().to_string(),
                prefix: prefix.to_string(),
            })?;
    let record = store
        .get(project, &work_id)
        .map_err(|error| QueryItemTagsError::ReadStore(Box::new(error)))?;
    Ok(record.map(|record| ItemTags {
        project: project.as_ref().to_string(),
        id: work_id.as_ref().to_string(),
        tags: record.tags,
    }))
}

#[cfg(test)]
mod tests {
    use pwf_domain::pending_work::{
        ProjectName, ProjectRegistry, Timestamp, WorkItemId, WorkItemStatus,
    };

    use super::{ItemTags, QueryItemTags, QueryItemTagsError, execute};
    use crate::{Materialization, PendingWorkItem, RecordId, testing::InMemoryStore};

    fn record(id: &str, status: WorkItemStatus, tags: Option<&str>) -> PendingWorkItem {
        PendingWorkItem {
            id: RecordId::Item(WorkItemId::try_new(id).unwrap()),
            title: "task".to_string(),
            status,
            created: Some(Timestamp::new("2026-07-07")),
            completed: None,
            commits: None,
            tags: tags.map(str::to_string),
            effort: None,
            prereq: None,
            section: None,
            body: String::new(),
            source: String::new(),
            locator: format!("/notes/pwf/{id}.md"),
            placement: None,
            materialization: Materialization::NoteFile,
        }
    }

    fn registry() -> ProjectRegistry {
        ProjectRegistry::new(vec![(
            ProjectName::try_new("pwf").unwrap(),
            Some("/repo/pwf".to_string()),
            Some("PWF".to_string()),
        )])
    }

    fn query(store: &InMemoryStore, id: &str) -> Result<Option<ItemTags>, QueryItemTagsError> {
        execute(&QueryItemTags { id: id.to_string() }, store, &registry())
    }

    #[test]
    fn reads_raw_tags_of_an_active_item() {
        let store = InMemoryStore::default().with_project(
            "pwf",
            vec![record(
                "PWF-0001",
                WorkItemStatus::Active,
                Some("[handoff]"),
            )],
        );

        let view = query(&store, "PWF-0001").unwrap().unwrap();

        assert_eq!(
            view,
            ItemTags {
                project: "pwf".to_string(),
                id: "PWF-0001".to_string(),
                tags: Some("[handoff]".to_string()),
            }
        );
    }

    #[test]
    fn reads_tags_of_a_closed_item_too() {
        let store = InMemoryStore::default().with_project(
            "pwf",
            vec![record("PWF-0001", WorkItemStatus::Done, Some("[handoff]"))],
        );

        let view = query(&store, "PWF-0001").unwrap().unwrap();

        assert_eq!(view.tags.as_deref(), Some("[handoff]"));
    }

    #[test]
    fn untagged_item_reads_none_tags_not_a_missing_item() {
        let store = InMemoryStore::default().with_project(
            "pwf",
            vec![record("PWF-0001", WorkItemStatus::Active, None)],
        );

        let view = query(&store, "PWF-0001").unwrap().unwrap();

        assert_eq!(view.tags, None);
    }

    #[test]
    fn canonical_id_lookup_is_case_insensitive() {
        let store = InMemoryStore::default().with_project(
            "pwf",
            vec![record(
                "PWF-0001",
                WorkItemStatus::Active,
                Some("[handoff]"),
            )],
        );

        assert_eq!(query(&store, "pwf-0001").unwrap().unwrap().id, "PWF-0001");
    }

    #[test]
    fn missing_item_reads_none_so_the_gate_skips() {
        let store = InMemoryStore::default().with_project("pwf", vec![]);

        assert!(query(&store, "PWF-9999").unwrap().is_none());
    }

    #[test]
    fn non_canonical_id_is_an_error_not_a_skip() {
        let store = InMemoryStore::default().with_project("pwf", vec![]);

        assert!(matches!(
            query(&store, "pwf:1").unwrap_err(),
            QueryItemTagsError::NotCanonical { .. }
        ));
    }

    #[test]
    fn unmanaged_prefix_is_an_error_not_a_skip() {
        let store = InMemoryStore::default().with_project("pwf", vec![]);

        assert!(matches!(
            query(&store, "XYZ-0001").unwrap_err(),
            QueryItemTagsError::UnknownPrefix { .. }
        ));
    }
}
