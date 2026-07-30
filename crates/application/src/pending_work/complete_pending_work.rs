use pwf_models::pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus};

mod queue;

use queue::{close_decisions, is_futuro_label};

use super::{
    add_pending_work_item::{
        AddPendingWorkError, AddPendingWorkItemOk, PendingWorkSection, added_item, project_mapped,
    },
    commit_provenance, identifier,
    note_body::append_report,
    project_registry::ProjectRegistry,
    store_util::{self, LoadItemError, body_region},
    title,
};
use crate::ports::{
    AppRecordStore, Clock, IndexEntry, IndexEntryState, IndexSection, ItemPatch, Materialization,
    NewItem, PendingWorkRecord,
};

#[derive(Debug, Clone)]
pub struct CompletePendingWork {
    pub id: String,
    pub date: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletePendingWorkOk {
    pub id: WorkItemId,
    pub project: ProjectName,
    pub title: String,
    pub action: ClosedItemAction,
    pub evicted_ids: Vec<WorkItemId>,
    pub futuro_renamed_project: Option<ProjectName>,
    pub review_item: Option<AddPendingWorkItemOk>,
}

#[derive(Debug, thiserror::Error)]
pub enum CompletePendingWorkError {
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
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
}

#[cqrsy::command]
pub fn execute<S, C>(
    command: &CompletePendingWork,
    store: &S,
    projects: &ProjectRegistry,
    clock: &C,
) -> Result<CompletePendingWorkOk, CompletePendingWorkError>
where
    S: AppRecordStore<PendingWorkRecord>
        + AppRecordStore<IndexEntry>
        + AppRecordStore<IndexSection>,
    C: Clock,
{
    let authored_date = command
        .date
        .clone()
        .map_or_else(|| clock.today(), Timestamp::new);
    perform_close(
        store,
        projects,
        ClosedItemAction::Done,
        &command.id,
        authored_date.as_str(),
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
) -> Result<CompletePendingWorkOk, CloseError>
where
    S: AppRecordStore<PendingWorkRecord>
        + AppRecordStore<IndexEntry>
        + AppRecordStore<IndexSection>,
{
    let commits_value = commit_provenance::normalize(commits);
    let pending_work_identifier =
        identifier::parse(id).ok_or_else(|| CloseError::ItemNotFound { id: id.to_string() })?;
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
    <S as AppRecordStore<PendingWorkRecord>>::update(
        store,
        project,
        &pending_work_identifier,
        patch,
    )
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

    Ok(CompletePendingWorkOk {
        id: pending_work_identifier,
        project: project.clone(),
        title,
        action,
        evicted_ids,
        futuro_renamed_project: futuro_renamed.then(|| project.clone()),
        review_item,
    })
}

fn map_load(error: LoadItemError) -> CloseError {
    match error {
        LoadItemError::ItemNotFound { id } => CloseError::ItemNotFound { id },
        LoadItemError::Store(source) => CloseError::WriteStore(source),
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
    S: AppRecordStore<IndexEntry> + AppRecordStore<IndexSection>,
{
    let entries = <S as AppRecordStore<IndexEntry>>::list(store, project)
        .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
    let sections = <S as AppRecordStore<IndexSection>>::list(store, project)
        .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
    let completed = Timestamp::new(completed);
    let decisions = close_decisions(&entries, &sections, id, &completed);

    if decisions.normalize_futuro_header {
        rename_futuro_headers(store, project, &sections)?;
    }
    if decisions.mark_target {
        <S as AppRecordStore<IndexEntry>>::update(
            store,
            project,
            id,
            IndexEntry {
                id: id.clone(),
                state: IndexEntryState::Done(completed),
                section: String::new(),
            },
        )
        .map_err(|error| CloseError::WriteStore(Box::new(error)))?;
    }
    for evicted in &decisions.evicted_ids {
        <S as AppRecordStore<IndexEntry>>::delete(store, project, evicted)
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
    S: AppRecordStore<IndexSection>,
{
    for section in sections.iter().filter(|s| is_futuro_label(&s.label)) {
        <S as AppRecordStore<IndexSection>>::update(
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
) -> Result<AddPendingWorkItemOk, CloseError>
where
    S: AppRecordStore<PendingWorkRecord>
        + AppRecordStore<IndexEntry>
        + AppRecordStore<IndexSection>,
{
    project_mapped(project.as_ref(), projects).map_err(CloseError::ReviewTask)?;
    let prompt = review_task_prompt(reviewed.as_ref(), commits);
    let review_title = title::inferred(&prompt)
        .map_err(AddPendingWorkError::from)
        .map_err(CloseError::ReviewTask)?;
    let created = store_util::create_item(
        store,
        project,
        NewItem {
            title: review_title,
            prompt,
            created: Timestamp::new(completed),
            section: Some(PendingWorkSection::Human.as_str().to_string()),
            prereq: None,
            effort: None,
            tags: None,
        },
    )
    .map_err(|source| {
        CloseError::ReviewTask(AddPendingWorkError::WriteStore {
            diagnostics: crate::pending_work::add_pending_work_item::AddPendingWorkDiagnostics {
                project: project.to_string(),
                created_section: source
                    .created_section()
                    .map(|(_, section)| section.to_string()),
            },
            source,
        })
    })?;
    Ok(added_item(project, created))
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
    use pwf_models::pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus};

    use super::{
        AddPendingWorkError, ClosedItemAction, CompletePendingWork, CompletePendingWorkError,
        ProjectRegistry, review_task_prompt,
    };
    use crate::{
        IndexEntry, IndexEntryState, Materialization, PendingWorkRecord, RecordId, ports::Clock,
        testing::InMemoryStore,
    };

    #[derive(Clone)]
    struct FixedClock;

    impl Clock for FixedClock {
        fn today(&self) -> Timestamp {
            Timestamp::new("2026-07-26")
        }
    }

    fn registry() -> ProjectRegistry {
        ProjectRegistry::new(vec![(
            ProjectName::try_new("foo-bar").unwrap(),
            Some("/repo".to_string()),
            Some("FOO".to_string()),
        )])
    }

    fn record(id: &str, status: WorkItemStatus) -> PendingWorkRecord {
        PendingWorkRecord {
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
            locator: format!("/mem/foo-bar/{id}.md"),
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

    fn foo() -> ProjectName {
        ProjectName::try_new("foo-bar").unwrap()
    }

    fn staged(items: Vec<PendingWorkRecord>, entries: Vec<IndexEntry>) -> InMemoryStore {
        let store = InMemoryStore::default()
            .with_prefix("foo-bar", "FOO")
            .with_project("foo-bar", items);
        for entry in entries {
            <InMemoryStore as crate::AppRecordStore<IndexEntry>>::insert(&store, &foo(), entry)
                .unwrap();
        }
        store
    }

    fn done_command(id: &str) -> CompletePendingWork {
        CompletePendingWork {
            id: id.to_string(),
            date: Some("2026-07-07".to_string()),
            report: None,
            commits: Vec::new(),
            review: false,
        }
    }

    #[test]
    fn done_marks_entry_and_evicts_past_cap() {
        let mut items = vec![record("FOO-0007", WorkItemStatus::Active)];
        let mut entries: Vec<IndexEntry> = (1..=6)
            .map(|n| {
                items.push(record(&format!("FOO-{n:04}"), WorkItemStatus::Done));
                entry(
                    &format!("FOO-{n:04}"),
                    IndexEntryState::Done(Timestamp::new(format!("2026-01-{n:02}"))),
                    "General",
                )
            })
            .collect();
        entries.push(entry("FOO-0007", IndexEntryState::Open, "General"));
        let store = staged(items, entries);

        let out =
            super::execute(&done_command("FOO-0007"), &store, &registry(), &FixedClock).unwrap();

        assert_eq!(out.action, ClosedItemAction::Done);
        assert_eq!(
            out.evicted_ids,
            vec![WorkItemId::try_new("FOO-0001").unwrap()]
        );
        assert_eq!(out.futuro_renamed_project, None);
        assert_eq!(store.items("foo-bar")[0].status, WorkItemStatus::Done);
        let marked = store
            .entries("foo-bar")
            .into_iter()
            .find(|e| e.id == WorkItemId::try_new("FOO-0007").unwrap())
            .unwrap();
        assert_eq!(
            marked.state,
            IndexEntryState::Done(Timestamp::new("2026-07-07"))
        );
        assert!(
            !store
                .entries("foo-bar")
                .iter()
                .any(|e| e.id == WorkItemId::try_new("FOO-0001").unwrap()),
            "evicted entry must be unlinked"
        );
    }

    #[test]
    fn done_uses_clock_date_when_no_date_is_explicit() {
        let store = staged(
            vec![record("FOO-0001", WorkItemStatus::Active)],
            vec![entry("FOO-0001", IndexEntryState::Open, "General")],
        );
        let mut command = done_command("FOO-0001");
        command.date = None;

        super::execute(&command, &store, &registry(), &FixedClock).unwrap();

        assert_eq!(
            store.items("foo-bar")[0].completed,
            Some(Timestamp::new("2026-07-26"))
        );
    }

    #[test]
    fn done_normalizes_futuro_header_entries() {
        let store = staged(
            vec![record("FOO-0001", WorkItemStatus::Active)],
            vec![entry("FOO-0001", IndexEntryState::Open, "Futuro")],
        );

        let out =
            super::execute(&done_command("FOO-0001"), &store, &registry(), &FixedClock).unwrap();

        assert_eq!(out.futuro_renamed_project, Some(foo()));
    }

    #[test]
    fn done_review_inserts_review_item_and_open_entry() {
        let store = staged(
            vec![record("FOO-0001", WorkItemStatus::Active)],
            vec![entry("FOO-0001", IndexEntryState::Open, "General")],
        );
        let cmd = CompletePendingWork {
            review: true,
            commits: vec!["a..b".to_string()],
            ..done_command("FOO-0001")
        };

        let out = super::execute(&cmd, &store, &registry(), &FixedClock).unwrap();

        let review = out.review_item.expect("review item present");
        assert_eq!(review.id, "FOO-0002");
        assert!(
            store
                .entries("foo-bar")
                .iter()
                .any(|e| e.id == WorkItemId::try_new("FOO-0002").unwrap()
                    && e.state == IndexEntryState::Open),
            "review task must get an open index entry"
        );
    }

    #[test]
    fn done_review_preserves_add_project_mapping_policy() {
        let store = staged(
            vec![record("FOO-0001", WorkItemStatus::Active)],
            vec![entry("FOO-0001", IndexEntryState::Open, "General")],
        );
        let projects = ProjectRegistry::new(vec![(foo(), None, Some("FOO".to_string()))]);
        let command = CompletePendingWork {
            review: true,
            ..done_command("FOO-0001")
        };

        let error = super::execute(&command, &store, &projects, &FixedClock).unwrap_err();

        assert!(matches!(
            error,
            CompletePendingWorkError::ReviewTask(
                AddPendingWorkError::ProjectHasNoDirectorySource { ref project }
            ) if project == "foo-bar"
        ));
        assert_eq!(
            error.to_string(),
            "Project 'foo-bar' has no directory source; update the managed project record."
        );
    }

    #[test]
    fn done_on_missing_item_reports_item_not_found() {
        let store = staged(Vec::new(), Vec::new());

        let error = super::execute(&done_command("FOO-9999"), &store, &registry(), &FixedClock)
            .unwrap_err();

        assert!(matches!(
            error,
            CompletePendingWorkError::ItemNotFound { ref id } if id == "FOO-9999"
        ));
        assert_eq!(
            error.to_string(),
            "Open pending-work item not found: FOO-9999"
        );
    }

    #[test]
    fn done_reports_an_unknown_configured_prefix() {
        let store = staged(Vec::new(), Vec::new());

        let error = super::execute(&done_command("XYZ-0001"), &store, &registry(), &FixedClock)
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown task id prefix `XYZ` for XYZ-0001"
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
