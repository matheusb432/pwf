use pwf_domain::pending_work::{ProjectRegistry, WorkItemId};

use crate::{AppDbStore, PendingWorkItem, RecordId};

#[derive(Debug, Clone)]
pub struct ResolvePendingWorkItem {
    pub id: String,
}

/// A resolved pending-work note: its display path and its full markdown source.
/// The caller (`pwf resolve` vs `pwf resolve --show`) picks which to emit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedItem {
    pub note_path: String,
    pub markdown: String,
}

impl From<PendingWorkItem> for ResolvedItem {
    fn from(record: PendingWorkItem) -> Self {
        Self {
            note_path: record.locator,
            markdown: record.source,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResolvePendingWorkError {
    /// Preserves the raw requested id verbatim (never re-normalized), matching
    /// the legacy infra `ItemNotFound` display.
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    /// A markdown stream was requested for an index wikilink whose note file is
    /// missing — there is no source to stream. Resolving the item's *path* is
    /// still fine; only `show`/`--show` reject this materialization.
    #[error("work-item note file is missing: {path}")]
    NoteFileMissing { path: String },
    #[error("{0}")]
    ReadStore(Box<dyn std::error::Error + Send + Sync>),
}

/// Resolves `id` to its stored record, open or closed, by mapping the id prefix
/// to its project and reading it from the generic store. A non-canonical id is
/// served by the inline-legacy scan (`<project>:<ordinal>` prompts have no
/// [`WorkItemId`], so they are found by listing and matching ordinals).
pub(crate) fn resolve_record<S>(
    store: &S,
    projects: &ProjectRegistry,
    id: &str,
) -> Result<PendingWorkItem, ResolvePendingWorkError>
where
    S: AppDbStore<PendingWorkItem>,
{
    let not_found = || ResolvePendingWorkError::ItemNotFound { id: id.to_string() };
    let Ok(work_id) = WorkItemId::try_new(id) else {
        return resolve_inline_record(store, projects, id);
    };
    let project = projects.project_for_id(&work_id).ok_or_else(not_found)?;
    store
        .get(project, &work_id)
        .map_err(|error| ResolvePendingWorkError::ReadStore(Box::new(error)))?
        .ok_or_else(not_found)
}

/// Finds the legacy inline record whose composed `<project>:<ordinal>` display
/// id matches `id` case-insensitively — the same match the legacy read applied
/// across every project's open items.
fn resolve_inline_record<S>(
    store: &S,
    projects: &ProjectRegistry,
    id: &str,
) -> Result<PendingWorkItem, ResolvePendingWorkError>
where
    S: AppDbStore<PendingWorkItem>,
{
    for (project, _repo) in projects.projects() {
        let records = store
            .list(project)
            .map_err(|error| ResolvePendingWorkError::ReadStore(Box::new(error)))?;
        let found = records.into_iter().find(|record| match record.id {
            RecordId::Inline(ordinal) => {
                format!("{}:{ordinal}", project.as_ref()).eq_ignore_ascii_case(id)
            }
            RecordId::Item(_) => false,
        });
        if let Some(record) = found {
            return Ok(record);
        }
    }
    Err(ResolvePendingWorkError::ItemNotFound { id: id.to_string() })
}

#[cqrsy::handler(query)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "the cqrsy resolve operation owns its request by contract"
)]
pub fn execute(
    query: ResolvePendingWorkItem,
    store: &impl AppDbStore<PendingWorkItem>,
    projects: &ProjectRegistry,
) -> Result<ResolvedItem, ResolvePendingWorkError> {
    resolve_record(store, projects, &query.id).map(ResolvedItem::from)
}

#[cfg(test)]
pub(crate) mod testing {
    use pwf_domain::pending_work::{
        ProjectName, ProjectRegistry, Timestamp, WorkItemId, WorkItemStatus,
    };

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

    /// A missing-note wikilink record ("ghost"): index entry exists, note
    /// file does not — empty body/source, locator = the expected note path.
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

    /// A legacy inline prompt record (``- [ ] `session` <- prompt`` index
    /// line): ordinal identity, prompt-as-source, index file as locator.
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
        ResolvePendingWorkError, ResolvePendingWorkItem, execute,
        testing::{staged, staged_ghost, staged_inline},
    };

    #[test]
    fn resolve_returns_path_and_markdown() {
        let (store, registry) = staged();

        let resolved = execute(
            ResolvePendingWorkItem {
                id: "PWF-0001".to_string(),
            },
            &store,
            &registry,
        )
        .unwrap();

        assert_eq!(resolved.note_path, "/notes/pwf/PWF-0001.md");
        assert_eq!(resolved.markdown, super::testing::PWF_0001_SOURCE);
    }

    #[test]
    fn resolve_returns_expected_note_path_for_missing_note_wikilink() {
        let (store, registry) = staged_ghost();

        let resolved = execute(
            ResolvePendingWorkItem {
                id: "PWF-0002".to_string(),
            },
            &store,
            &registry,
        )
        .unwrap();

        // The path form still resolves a ghost item (legacy parity); only the
        // markdown/show form rejects it.
        assert_eq!(resolved.note_path, "/notes/pwf/PWF-0002.md");
    }

    #[test]
    fn resolve_serves_inline_legacy_id_case_insensitively() {
        let (store, registry) = staged_inline();

        let resolved = execute(
            ResolvePendingWorkItem {
                id: "PWF:1".to_string(),
            },
            &store,
            &registry,
        )
        .unwrap();

        // Path form → the index note holding the inline prompt; markdown form
        // → the prompt itself (no frontmatter to strip).
        assert_eq!(resolved.note_path, "/notes/pwf/pwf.md");
        assert_eq!(resolved.markdown, "do the legacy thing");
    }

    #[test]
    fn resolve_unknown_inline_id_preserves_raw_id() {
        let (store, registry) = staged_inline();

        let error = execute(
            ResolvePendingWorkItem {
                id: "pwf:9".to_string(),
            },
            &store,
            &registry,
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "Open pending-work item not found: pwf:9");
    }

    #[test]
    fn resolve_missing_id_preserves_raw_lowercase_id() {
        let (store, registry) = staged();

        let error = execute(
            ResolvePendingWorkItem {
                id: "pwf-9999".to_string(),
            },
            &store,
            &registry,
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Open pending-work item not found: pwf-9999"
        );
        assert!(matches!(
            error,
            ResolvePendingWorkError::ItemNotFound { id } if id == "pwf-9999"
        ));
    }
}
