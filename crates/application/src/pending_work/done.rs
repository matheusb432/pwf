use pwf_domain::pending_work::{
    MarkedEntry, ProjectName, QueueEntryView, Timestamp, WorkItemId, WorkItemStatus,
    close_decisions, is_futuro_label,
};

use super::{
    add::{AddPendingWorkError, AddedItem, PendingWorkSection, added_item, project_mapped},
    commit_provenance,
    note_body::append_report,
    project_registry::ProjectRegistry,
    store_util::{self, LoadItemError, body_region},
};
use crate::{
    handoff::{HandoffError, HandoffMutationOk, lifecycle},
    ports::{
        AppDbStore, HandoffDocumentStore, HandoffLedger, IndexEntry, IndexEntryState, IndexSection,
        ItemPatch, Materialization, NewItem, PendingWorkItem,
    },
};

#[derive(Debug, Clone)]
pub struct CompletePendingWork {
    pub id: String,
    pub completed: String,
    pub report: Option<String>,
    pub commits: Vec<String>,
    pub review: bool,
}

/// Selects the status and confirmation verb recorded by a close operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosedItemAction {
    Done,
    Cancelled,
}

impl ClosedItemAction {
    #[must_use]
    pub fn past_tense(self) -> &'static str {
        match self {
            Self::Done => "Done",
            Self::Cancelled => "Cancelled",
        }
    }

    fn status(self) -> WorkItemStatus {
        match self {
            Self::Done => WorkItemStatus::Done,
            Self::Cancelled => WorkItemStatus::Cancelled,
        }
    }
}

/// Contains a closed item's identity and queue side effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedPendingWork {
    pub id: WorkItemId,
    pub project: ProjectName,
    pub title: String,
    pub action: ClosedItemAction,
    pub evicted_ids: Vec<WorkItemId>,
    pub futuro_renamed_project: Option<ProjectName>,
    pub review_item: Option<AddedItem>,
    pub handoff: HandoffMutationOk,
}

#[derive(Debug, thiserror::Error)]
pub enum CompletePendingWorkError {
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    /// Reports a canonical identifier whose prefix has no configured project.
    #[error("Unknown task id prefix `{prefix}` for {pending_work_identifier}")]
    UnknownPrefix {
        pending_work_identifier: String,
        prefix: String,
    },
    #[error("--report cannot be empty.")]
    EmptyReport,
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    ReviewTask(#[source] AddPendingWorkError),
    #[error(transparent)]
    HandoffPreflight(HandoffError),
    #[error("{source}")]
    HandoffAfterPendingWork {
        pending_work_identifier: WorkItemId,
        completed: Box<CompletedPendingWork>,
        #[source]
        source: HandoffError,
    },
}

#[cqrsy::command]
pub fn execute<S>(
    command: &CompletePendingWork,
    store: &S,
    projects: &ProjectRegistry,
) -> Result<CompletedPendingWork, CompletePendingWorkError>
where
    S: AppDbStore<PendingWorkItem>
        + AppDbStore<IndexEntry>
        + AppDbStore<IndexSection>
        + HandoffDocumentStore
        + AppDbStore<HandoffLedger>,
{
    perform_close(
        store,
        projects,
        ClosedItemAction::Done,
        &command.id,
        &command.completed,
        command.report.as_deref(),
        &command.commits,
        command.review,
    )
    .map_err(CloseError::into_complete)
}

/// Reports failures shared by the done and cancel operations.
pub(super) enum CloseError {
    ItemNotFound {
        id: String,
    },
    UnknownPrefix {
        pending_work_identifier: String,
        prefix: String,
    },
    EmptyReport,
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
    ReviewTask(AddPendingWorkError),
    HandoffPreflight(HandoffError),
    HandoffAfterPendingWork {
        pending_work_identifier: WorkItemId,
        completed: Box<CompletedPendingWork>,
        source: HandoffError,
    },
}

impl CloseError {
    fn into_complete(self) -> CompletePendingWorkError {
        match self {
            Self::ItemNotFound { id } => CompletePendingWorkError::ItemNotFound { id },
            Self::UnknownPrefix {
                pending_work_identifier,
                prefix,
            } => CompletePendingWorkError::UnknownPrefix {
                pending_work_identifier,
                prefix,
            },
            Self::EmptyReport => CompletePendingWorkError::EmptyReport,
            Self::WriteStore(source) => CompletePendingWorkError::WriteStore(source),
            Self::ReviewTask(source) => CompletePendingWorkError::ReviewTask(source),
            Self::HandoffPreflight(source) => CompletePendingWorkError::HandoffPreflight(source),
            Self::HandoffAfterPendingWork {
                pending_work_identifier,
                completed,
                source,
            } => CompletePendingWorkError::HandoffAfterPendingWork {
                pending_work_identifier,
                completed,
                source,
            },
        }
    }
}

/// Closes an item through the flow shared by done and cancel.
///
/// One patch applies report and status fields. Only note-backed records rotate the queue, because
/// inline and missing-note records have no file-backed queue entry.
#[expect(
    clippy::too_many_arguments,
    reason = "done and cancel share this orchestration"
)]
pub(super) fn perform_close<S>(
    store: &S,
    projects: &ProjectRegistry,
    action: ClosedItemAction,
    id: &str,
    completed: &str,
    report: Option<&str>,
    commits: &[String],
    review: bool,
) -> Result<CompletedPendingWork, CloseError>
where
    S: AppDbStore<PendingWorkItem>
        + AppDbStore<IndexEntry>
        + AppDbStore<IndexSection>
        + HandoffDocumentStore
        + AppDbStore<HandoffLedger>,
{
    let commits_value = commit_provenance::normalize(commits);
    let pending_work_identifier =
        WorkItemId::try_new(id).map_err(|_| CloseError::ItemNotFound { id: id.to_string() })?;
    let prefix = pending_work_identifier
        .as_ref()
        .split_once('-')
        .map_or("", |(prefix, _)| prefix);
    let project = projects
        .project_for_id(&pending_work_identifier)
        .ok_or_else(|| CloseError::UnknownPrefix {
            pending_work_identifier: pending_work_identifier.to_string(),
            prefix: prefix.to_string(),
        })?;
    let record =
        store_util::require_item(store, project, &pending_work_identifier).map_err(map_load)?;
    if record.status != WorkItemStatus::Active {
        return Err(CloseError::ItemNotFound {
            id: pending_work_identifier.as_ref().to_string(),
        });
    }
    let title = record.title.clone();
    let handoff_pending = lifecycle::preflight_close(
        store,
        projects,
        pending_work_identifier.as_ref(),
        match action {
            ClosedItemAction::Done => lifecycle::CloseHandoffAction::Done,
            ClosedItemAction::Cancelled => lifecycle::CloseHandoffAction::Cancelled,
        },
        completed,
        report,
    )
    .map_err(CloseError::HandoffPreflight)?;

    let mut patch = ItemPatch {
        status: Some(action.status()),
        completed: Some(Some(Timestamp::new(completed))),
        ..ItemPatch::default()
    };
    if let Some(report) = report {
        let body =
            append_report(body_region(&record.body), report).ok_or(CloseError::EmptyReport)?;
        patch.body = Some(body);
    }
    if let Some(commits) = &commits_value {
        patch.commits = Some(Some(commits.clone()));
    }
    <S as AppDbStore<PendingWorkItem>>::update(store, project, &pending_work_identifier, patch)
        .map_err(|error| CloseError::WriteStore(Box::new(error)))?;

    let (evicted_ids, futuro_renamed) =
        if matches!(record.materialization, Materialization::NoteFile) {
            rotate_done_queue(store, project, &pending_work_identifier, completed)?
        } else {
            (Vec::new(), false)
        };

    let review_item = review
        .then(|| {
            spawn_review(
                store,
                projects,
                project,
                &pending_work_identifier,
                completed,
                commits_value.as_deref(),
            )
        })
        .transpose()?;

    let mut completed_pending_work = CompletedPendingWork {
        id: pending_work_identifier,
        project: project.clone(),
        title,
        action,
        evicted_ids,
        futuro_renamed_project: futuro_renamed.then(|| project.clone()),
        review_item,
        handoff: HandoffMutationOk::NotLinked,
    };
    completed_pending_work.handoff = lifecycle::commit_after_pending_work(store, handoff_pending)
        .map_err(|source| CloseError::HandoffAfterPendingWork {
        pending_work_identifier: completed_pending_work.id.clone(),
        completed: Box::new(completed_pending_work.clone()),
        source,
    })?;
    Ok(completed_pending_work)
}

fn map_load(error: LoadItemError) -> CloseError {
    match error {
        LoadItemError::ItemNotFound { id } => CloseError::ItemNotFound { id },
        LoadItemError::Store(source) => CloseError::WriteStore(source),
    }
}

/// Maps an index entry to queue semantics while preserving raw section labels.
///
/// An empty label becomes the `"General"` no-header sentinel.
pub(super) fn queue_view(entry: &IndexEntry) -> QueueEntryView {
    let completed = match &entry.state {
        IndexEntryState::Open => None,
        IndexEntryState::Done(date) => Some(date.clone()),
    };
    let section = if entry.section.is_empty() {
        "General".to_string()
    } else {
        entry.section.clone()
    };
    QueueEntryView {
        id: entry.id.clone(),
        completed,
        section,
    }
}

/// Applies header normalization, the closed entry, and cap-based evictions to the index.
fn rotate_done_queue<S>(
    store: &S,
    project: &ProjectName,
    id: &WorkItemId,
    completed: &str,
) -> Result<(Vec<WorkItemId>, bool), CloseError>
where
    S: AppDbStore<IndexEntry> + AppDbStore<IndexSection>,
{
    let entries = <S as AppDbStore<IndexEntry>>::list(store, project)
        .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
    let sections = <S as AppDbStore<IndexSection>>::list(store, project)
        .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
    let views: Vec<QueueEntryView> = entries.iter().map(queue_view).collect();
    let labels: Vec<String> = sections
        .iter()
        .map(|section| section.label.clone())
        .collect();
    let decisions = close_decisions(&views, &labels, id, &Timestamp::new(completed));

    if decisions.normalize_futuro_header {
        rename_futuro_headers(store, project, &sections)?;
    }
    if let Some(MarkedEntry {
        id: marked_id,
        completed,
        ..
    }) = &decisions.marked_entry
    {
        <S as AppDbStore<IndexEntry>>::update(
            store,
            project,
            marked_id,
            IndexEntry {
                id: marked_id.clone(),
                state: IndexEntryState::Done(completed.clone()),
                section: String::new(),
            },
        )
        .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
    }
    for evicted in &decisions.evicted_ids {
        <S as AppDbStore<IndexEntry>>::delete(store, project, evicted)
            .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
    }
    Ok((decisions.evicted_ids, decisions.normalize_futuro_header))
}

/// Renames every `## Futuro` header to `## Future` through the section port.
fn rename_futuro_headers<S>(
    store: &S,
    project: &ProjectName,
    sections: &[IndexSection],
) -> Result<(), CloseError>
where
    S: AppDbStore<IndexSection>,
{
    for section in sections.iter().filter(|s| is_futuro_label(&s.label)) {
        <S as AppDbStore<IndexSection>>::update(
            store,
            project,
            &section.label,
            IndexSection {
                label: "Future".to_string(),
            },
        )
        .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
    }
    Ok(())
}

fn spawn_review<S>(
    store: &S,
    projects: &ProjectRegistry,
    project: &ProjectName,
    reviewed: &WorkItemId,
    completed: &str,
    commits: Option<&str>,
) -> Result<AddedItem, CloseError>
where
    S: AppDbStore<PendingWorkItem> + AppDbStore<IndexEntry> + AppDbStore<IndexSection>,
{
    project_mapped(project.as_ref(), projects).map_err(CloseError::ReviewTask)?;
    let created = store_util::create_item(
        store,
        project,
        NewItem {
            prompt: review_task_prompt(reviewed.as_ref(), commits),
            title: None,
            created: Timestamp::new(completed),
            section: Some(PendingWorkSection::Human.as_str().to_string()),
            prereq: None,
            effort: None,
            tags: None,
        },
    )
    .map_err(|source| {
        CloseError::ReviewTask(AddPendingWorkError::WriteStore {
            diagnostics: crate::pending_work::add::AddPendingWorkDiagnostics {
                project: project.to_string(),
                created_section: source
                    .created_section()
                    .map(|(_, section)| section.to_string()),
                title_normalized: false,
            },
            source,
        })
    })?;
    Ok(added_item(
        project,
        created,
        false,
        crate::handoff::HandoffMutationOk::NotLinked,
    ))
}

pub(super) fn review_task_prompt(reviewed_id: &str, range: Option<&str>) -> String {
    let (title, diff) = match range {
        Some(range) => (
            format!("review {reviewed_id}, commits: {range}"),
            format!("git-tools diff {range}"),
        ),
        None => (
            format!("review {reviewed_id}"),
            "git-tools diff".to_string(),
        ),
    };
    format!("{title} / {diff} / git-tools diff-subrepos")
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::SystemTime};

    use pwf_domain::{
        handoff::HandoffStatus,
        pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus},
    };

    use super::{
        AddPendingWorkError, ClosedItemAction, CompletePendingWork, CompletePendingWorkError,
        ProjectRegistry, execute, review_task_prompt,
    };
    use crate::{
        HandoffDocument, HandoffDocumentIdentifier, HandoffLocation, HandoffScope, IndexEntry,
        IndexEntryState, Materialization, PendingWorkItem, RecordId,
        handoff::HandoffMutationOk,
        testing::{FailurePoint, InMemoryStore},
    };

    fn registry() -> ProjectRegistry {
        ProjectRegistry::new(vec![(
            ProjectName::try_new("glep-shimeji").unwrap(),
            Some("/repo".to_string()),
            Some("GLP".to_string()),
        )])
    }

    fn record(id: &str, status: WorkItemStatus) -> PendingWorkItem {
        PendingWorkItem {
            id: RecordId::Item(WorkItemId::try_new(id).unwrap()),
            title: "tray gui".to_string(),
            status,
            created: Some(Timestamp::new("2026-01-01")),
            completed: None,
            commits: None,
            tags: None,
            effort: None,
            prereq: None,
            section: None,
            body: "\nbody\n".to_string(),
            source: "body".to_string(),
            locator: format!("/mem/glep-shimeji/{id}.md"),
            placement: None,
            materialization: Materialization::NoteFile,
        }
    }

    fn entry(id: &str, state: IndexEntryState, section: &str) -> IndexEntry {
        IndexEntry {
            id: WorkItemId::try_new(id).unwrap(),
            state,
            section: section.to_string(),
        }
    }

    fn glp() -> ProjectName {
        ProjectName::try_new("glep-shimeji").unwrap()
    }

    fn staged(items: Vec<PendingWorkItem>, entries: Vec<IndexEntry>) -> InMemoryStore {
        let store = InMemoryStore::default()
            .with_prefix("glep-shimeji", "GLP")
            .with_project("glep-shimeji", items);
        for entry in entries {
            <InMemoryStore as crate::AppDbStore<IndexEntry>>::insert(&store, &glp(), entry)
                .unwrap();
        }
        store
    }

    fn done_command(id: &str) -> CompletePendingWork {
        CompletePendingWork {
            id: id.to_string(),
            completed: "2026-07-07".to_string(),
            report: None,
            commits: Vec::new(),
            review: false,
        }
    }

    fn handoff() -> HandoffDocument {
        HandoffDocument {
            identifier: HandoffDocumentIdentifier {
                file_name: "2026-01-01-tray-gui.md".to_string(),
                location: HandoffLocation::Active,
            },
            location: HandoffLocation::Active,
            project: Some(glp()),
            title: "Tray GUI".to_string(),
            status: Some(HandoffStatus::Active),
            created: Some(Timestamp::new("2026-01-01")),
            completed: None,
            pending_work_identifier_raw: Some("glp-0001".to_string()),
            goals_completed: 0,
            goals_total: 1,
            body: "\n# Tray GUI\n".to_string(),
            source: "---\nstatus: active\nproject: glep-shimeji\ncreated: 2026-01-01\npw: glp-0001\n---\n\n# Tray GUI\n".to_string(),
            locator: PathBuf::from("/repo/docs/handoffs/2026-01-01-tray-gui.md"),
            modified_timestamp: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn done_marks_entry_and_evicts_past_cap() {
        let mut items = vec![record("GLP-0007", WorkItemStatus::Active)];
        let mut entries: Vec<IndexEntry> = (1..=6)
            .map(|n| {
                items.push(record(&format!("GLP-{n:04}"), WorkItemStatus::Done));
                entry(
                    &format!("GLP-{n:04}"),
                    IndexEntryState::Done(Timestamp::new(format!("2026-01-{n:02}"))),
                    "General",
                )
            })
            .collect();
        entries.push(entry("GLP-0007", IndexEntryState::Open, "General"));
        let store = staged(items, entries);

        let out = execute(&done_command("GLP-0007"), &store, &registry()).unwrap();

        assert_eq!(out.action, ClosedItemAction::Done);
        assert_eq!(
            out.evicted_ids,
            vec![WorkItemId::try_new("GLP-0001").unwrap()]
        );
        assert_eq!(out.futuro_renamed_project, None);
        assert_eq!(store.items("glep-shimeji")[0].status, WorkItemStatus::Done);
        let marked = store
            .entries("glep-shimeji")
            .into_iter()
            .find(|e| e.id == WorkItemId::try_new("GLP-0007").unwrap())
            .unwrap();
        assert_eq!(
            marked.state,
            IndexEntryState::Done(Timestamp::new("2026-07-07"))
        );
        assert!(
            !store
                .entries("glep-shimeji")
                .iter()
                .any(|e| e.id == WorkItemId::try_new("GLP-0001").unwrap()),
            "evicted entry must be unlinked"
        );
    }

    #[test]
    fn done_normalizes_futuro_header_entries() {
        let store = staged(
            vec![record("GLP-0001", WorkItemStatus::Active)],
            vec![entry("GLP-0001", IndexEntryState::Open, "Futuro")],
        );

        let out = execute(&done_command("GLP-0001"), &store, &registry()).unwrap();

        assert_eq!(out.futuro_renamed_project, Some(glp()));
    }

    #[test]
    fn done_review_inserts_review_item_and_open_entry() {
        let store = staged(
            vec![record("GLP-0001", WorkItemStatus::Active)],
            vec![entry("GLP-0001", IndexEntryState::Open, "General")],
        );
        let cmd = CompletePendingWork {
            review: true,
            commits: vec!["a..b".to_string()],
            ..done_command("GLP-0001")
        };

        let out = execute(&cmd, &store, &registry()).unwrap();

        let review = out.review_item.expect("review item present");
        assert_eq!(review.id, "GLP-0002");
        assert!(
            store
                .entries("glep-shimeji")
                .iter()
                .any(|e| e.id == WorkItemId::try_new("GLP-0002").unwrap()
                    && e.state == IndexEntryState::Open),
            "review task must get an open index entry"
        );
    }

    #[test]
    fn done_review_preserves_add_project_mapping_policy() {
        let store = staged(
            vec![record("GLP-0001", WorkItemStatus::Active)],
            vec![entry("GLP-0001", IndexEntryState::Open, "General")],
        );
        let projects = ProjectRegistry::new(vec![(glp(), None, Some("GLP".to_string()))]);
        let command = CompletePendingWork {
            review: true,
            ..done_command("GLP-0001")
        };

        let error = execute(&command, &store, &projects).unwrap_err();

        assert!(matches!(
            error,
            CompletePendingWorkError::ReviewTask(
                AddPendingWorkError::ProjectNotMappedToRepo { ref project }
            ) if project == "glep-shimeji"
        ));
        assert_eq!(
            error.to_string(),
            "Project 'glep-shimeji' is not mapped to a repo in config/pending-work.json."
        );
    }

    #[test]
    fn done_on_missing_item_reports_item_not_found() {
        let store = staged(Vec::new(), Vec::new());

        let error = execute(&done_command("GLP-9999"), &store, &registry()).unwrap_err();

        assert!(matches!(
            error,
            CompletePendingWorkError::ItemNotFound { ref id } if id == "GLP-9999"
        ));
        assert_eq!(
            error.to_string(),
            "Open pending-work item not found: GLP-9999"
        );
    }

    #[test]
    fn done_reports_an_unknown_configured_prefix() {
        let store = staged(Vec::new(), Vec::new());

        let error = execute(&done_command("XYZ-0001"), &store, &registry()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown task id prefix `XYZ` for XYZ-0001"
        );
    }

    #[test]
    fn done_archives_the_linked_handoff_after_closing_pending_work() {
        let tagged = PendingWorkItem {
            tags: Some("[handoff]".to_string()),
            ..record("GLP-0001", WorkItemStatus::Active)
        };
        let scope = HandoffScope {
            repository_root: PathBuf::from("/repo"),
        };
        let store = staged(
            vec![tagged],
            vec![entry("GLP-0001", IndexEntryState::Open, "General")],
        )
        .with_handoff_documents(scope.clone(), vec![handoff()]);

        let outcome = execute(&done_command("GLP-0001"), &store, &registry()).unwrap();

        assert!(matches!(
            outcome.handoff,
            HandoffMutationOk::Archived { .. }
        ));
        assert_eq!(store.items("glep-shimeji")[0].status, WorkItemStatus::Done);
        assert_eq!(
            store.handoff_documents(&scope)[0].location,
            HandoffLocation::Archived
        );
    }

    #[test]
    fn done_reports_handoff_failure_after_pending_work_is_closed() {
        let tagged = PendingWorkItem {
            tags: Some("[handoff]".to_string()),
            ..record("GLP-0001", WorkItemStatus::Active)
        };
        let scope = HandoffScope {
            repository_root: PathBuf::from("/repo"),
        };
        let store = staged(
            vec![tagged],
            vec![entry("GLP-0001", IndexEntryState::Open, "General")],
        )
        .with_handoff_documents(scope, vec![handoff()])
        .with_failure(FailurePoint::DocumentUpdate);

        let error = execute(&done_command("GLP-0001"), &store, &registry()).unwrap_err();

        assert!(matches!(
            error,
            CompletePendingWorkError::HandoffAfterPendingWork {
                ref pending_work_identifier,
                ..
            } if pending_work_identifier.as_ref() == "GLP-0001"
        ));
        assert_eq!(store.items("glep-shimeji")[0].status, WorkItemStatus::Done);
    }

    #[test]
    fn done_handoff_preflight_precedes_blank_report_validation() {
        let tagged = PendingWorkItem {
            tags: Some("[handoff]".to_string()),
            ..record("GLP-0001", WorkItemStatus::Active)
        };
        let store = staged(
            vec![tagged],
            vec![entry("GLP-0001", IndexEntryState::Open, "General")],
        );
        let command = CompletePendingWork {
            report: Some(" \t\n".to_string()),
            ..done_command("GLP-0001")
        };

        let error = execute(&command, &store, &registry()).unwrap_err();

        assert!(matches!(
            error,
            CompletePendingWorkError::HandoffPreflight(_)
        ));
        assert_eq!(
            store.items("glep-shimeji")[0].status,
            WorkItemStatus::Active
        );
    }

    #[test]
    fn review_prompt_uses_scoped_or_bare_diff() {
        assert_eq!(
            review_task_prompt("PWF-0128", Some("a..b")),
            "review PWF-0128, commits: a..b / git-tools diff a..b / git-tools diff-subrepos"
        );
        assert_eq!(
            review_task_prompt("PWF-0128", None),
            "review PWF-0128 / git-tools diff / git-tools diff-subrepos"
        );
    }
}
