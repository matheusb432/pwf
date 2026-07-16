use pwf_domain::pending_work::{ProjectRegistry, WorkItemId};

use crate::{AppDbStore, PendingWorkItem};

#[derive(Debug, Clone)]
pub struct QueryItemTags {
    pub id: String,
}

/// An item's owning project, canonical id, and raw `tags:` frontmatter — the
/// minimal view the handoff mirror gate needs to decide whether a close/remove/
/// reopen must mirror onto a linked handoff file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemTags {
    pub project: String,
    pub id: String,
    /// Raw `tags:` frontmatter, verbatim and unparsed — the caller validates it
    /// (so a corrupt value surfaces the caller's own error, not this query's).
    pub tags: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum QueryItemTagsError {
    /// The requested id is not a canonical [`WorkItemId`]. The gate treats this
    /// as an error (not a silent skip), matching the legacy note lookup.
    #[error("Open pending-work item not found: {id}")]
    NotCanonical { id: String },
    /// The id's prefix maps to no managed project.
    #[error("Unknown task id prefix `{prefix}` for {id}")]
    UnknownPrefix { id: String, prefix: String },
    #[error("{0}")]
    ReadStore(Box<dyn std::error::Error + Send + Sync>),
}

/// Reads the tags of the item identified by `id`, open **or** closed, by mapping
/// the id prefix to its project and reading the record from the generic store.
/// `Ok(None)` means the id is canonical and its prefix is managed but no matching
/// record exists — the same "no item to gate" signal the mirror gate turned into
/// a skip. A non-canonical id or an unmanaged prefix is an error, preserving the
/// legacy note-lookup's error-vs-none boundary exactly.
#[cqrsy::handler(query)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "the cqrsy query operation owns its request by contract"
)]
pub fn execute(
    query: QueryItemTags,
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
        execute(QueryItemTags { id: id.to_string() }, store, &registry())
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
