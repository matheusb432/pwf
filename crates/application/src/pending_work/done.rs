use pwf_domain::pending_work::{
    AddedItem, MarkedEntry, ProjectName, ProjectRegistry, QueueEntryView, Timestamp, WorkItemId,
    WorkItemStatus, append_report, close_decisions, is_futuro_label,
};

use super::{
    add::{AddPendingWorkError, AddPendingWorkItem},
    store_util::{self, LoadItemError, body_region},
};
use crate::ports::{
    AppDbStore, IndexEntry, IndexEntryState, IndexSection, ItemPatch, Materialization,
    PendingWorkItem,
};

#[derive(Debug, Clone)]
pub struct CompletePendingWork {
    pub id: String,
    pub completed: String,
    pub report: Option<String>,
    pub commits: Vec<String>,
    pub review: bool,
}

/// Which status transition a close records. Owns the past-tense verb rendered in
/// the CLI close confirmation (the single source for `Done`/`Cancelled`).
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

/// The result of closing (done/cancel) a pending-work item: the closed item's
/// identity plus the done-queue side effects the CLI renders and diagnoses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedPendingWork {
    pub id: WorkItemId,
    pub project: ProjectName,
    pub title: String,
    pub action: ClosedItemAction,
    pub evicted_ids: Vec<WorkItemId>,
    pub futuro_renamed_project: Option<ProjectName>,
    pub review_item: Option<AddedItem>,
}

#[derive(Debug, thiserror::Error)]
pub enum CompletePendingWorkError {
    /// Verbatim former infra `ItemNotFound` display (PWF-0123 error-string
    /// relocation) — also covers an already-closed item: only open items close.
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    #[error("--report cannot be empty.")]
    EmptyReport,
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    ReviewTask(#[source] AddPendingWorkError),
}

#[cqrsy::handler(command)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "the cqrsy done operation owns its request by contract"
)]
pub fn execute<S>(
    command: CompletePendingWork,
    store: &S,
    projects: &ProjectRegistry,
) -> Result<CompletedPendingWork, CompletePendingWorkError>
where
    S: AppDbStore<PendingWorkItem> + AppDbStore<IndexEntry> + AppDbStore<IndexSection>,
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

/// Error surface shared by the `done` and `cancel` orchestrators — each handler
/// maps it 1:1 onto its own public error enum so the two flows stay one code
/// path (`perform_close`) with no duplication.
pub(super) enum CloseError {
    ItemNotFound { id: String },
    EmptyReport,
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
    ReviewTask(AddPendingWorkError),
}

impl CloseError {
    fn into_complete(self) -> CompletePendingWorkError {
        match self {
            Self::ItemNotFound { id } => CompletePendingWorkError::ItemNotFound { id },
            Self::EmptyReport => CompletePendingWorkError::EmptyReport,
            Self::WriteStore(source) => CompletePendingWorkError::WriteStore(source),
            Self::ReviewTask(source) => CompletePendingWorkError::ReviewTask(source),
        }
    }
}

/// The compound close flow shared by `done` and `cancel`: resolve the project,
/// require an open item, patch its note (report + status/completed/commits) in a
/// single [`ItemPatch`], then — only for a note-backed record
/// ([`Materialization::NoteFile`]) — rotate the done queue via the pure domain
/// [`close_decisions`], and spawn the optional `--review` task. The status patch
/// is uniform across storage models; the queue rotation is the one
/// materialization-gated step, since an inline or missing-note record has no
/// file-model done queue to rotate.
#[expect(
    clippy::too_many_arguments,
    reason = "the two closing verbs share one flattened flow rather than duplicate it"
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
    S: AppDbStore<PendingWorkItem> + AppDbStore<IndexEntry> + AppDbStore<IndexSection>,
{
    let commits_value = frontmatter_value(commits);
    let wid =
        WorkItemId::try_new(id).map_err(|_| CloseError::ItemNotFound { id: id.to_string() })?;
    let project = projects
        .project_for_id(&wid)
        .ok_or_else(|| CloseError::ItemNotFound { id: id.to_string() })?;
    let record = store_util::require_item(store, project, &wid).map_err(map_load)?;
    if record.status != WorkItemStatus::Active {
        return Err(CloseError::ItemNotFound {
            id: wid.as_ref().to_string(),
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
    <S as AppDbStore<PendingWorkItem>>::update(store, project, &wid, patch)
        .map_err(|error| CloseError::WriteStore(Box::new(error)))?;

    let (evicted_ids, futuro_renamed) =
        if matches!(record.materialization, Materialization::NoteFile) {
            rotate_done_queue(store, project, &wid, completed)?
        } else {
            (Vec::new(), false)
        };

    let review_item = review
        .then(|| {
            spawn_review(
                store,
                projects,
                project,
                &wid,
                completed,
                commits_value.as_deref(),
            )
        })
        .transpose()?;

    Ok(CompletedPendingWork {
        id: wid,
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

/// The `QueueEntryView` for an index entry, mapping the adapter's raw section
/// label onto the domain's done-queue section semantics: a headerless-region
/// entry (empty raw label) becomes the `"General"` sentinel — mirroring the
/// legacy `section_at_line` fallback exactly — while any real header label is
/// passed through raw for the domain to canonicalize. A real `## General`
/// header collapses to the same `"General"` the sentinel does, which is the same
/// ambiguity the legacy port carried, so parity holds.
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

/// Applies the done-queue decisions to the project index: rename a legacy
/// `## Futuro` header, mark the closed entry done, and evict the oldest links
/// beyond the section cap. Returns `(evicted_ids, futuro_renamed)`.
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

/// Renames every `## Futuro` header region to `## Future` via the section-label
/// `update` seam — the document-wide rename the legacy `mark_done` performed
/// textually before eviction (representation-only, PWF-0123).
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
    super::add::execute(
        AddPendingWorkItem {
            project_name: project.as_ref().to_string(),
            prompt: review_task_prompt(reviewed.as_ref(), commits),
            title: None,
            created: completed.to_string(),
            section: Some("Human".to_string()),
            prereq: None,
            effort: None,
            tags: None,
        },
        store,
        projects,
    )
    .map_err(CloseError::ReviewTask)
}

pub(super) fn frontmatter_value(values: &[String]) -> Option<String> {
    let mut ranges: Vec<String> = Vec::new();
    for value in values {
        for raw in value.split(',') {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            if !ranges.iter().any(|range| range == raw) {
                ranges.push(raw.to_string());
            }
        }
    }
    (!ranges.is_empty()).then(|| ranges.join(", "))
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
    use pwf_domain::pending_work::{
        ProjectName, ProjectRegistry, Timestamp, WorkItemId, WorkItemStatus,
    };

    use super::{
        ClosedItemAction, CompletePendingWork, CompletePendingWorkError, execute,
        frontmatter_value, review_task_prompt,
    };
    use crate::{
        IndexEntry, IndexEntryState, Materialization, PendingWorkItem, RecordId,
        testing::InMemoryStore,
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

        let out = execute(done_command("GLP-0007"), &store, &registry()).unwrap();

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

        let out = execute(done_command("GLP-0001"), &store, &registry()).unwrap();

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

        let out = execute(cmd, &store, &registry()).unwrap();

        // The in-memory double allocates the id but does not infer the note
        // title (that is the vault adapter's job, pinned by the CLI e2e); this
        // test owns the "review item + open index entry inserted" contract.
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
    fn done_on_missing_item_reports_item_not_found() {
        let store = staged(Vec::new(), Vec::new());

        let error = execute(done_command("GLP-9999"), &store, &registry()).unwrap_err();

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
    fn commit_ranges_trim_split_and_deduplicate_in_first_seen_order() {
        let values: Vec<String> = [" a..b,c..d ", "a..b", " e..f "]
            .iter()
            .map(|value| (*value).to_string())
            .collect();
        assert_eq!(
            frontmatter_value(&values),
            Some("a..b, c..d, e..f".to_string())
        );
        assert_eq!(
            frontmatter_value(&[String::new(), "  ".to_string(), ",".to_string()]),
            None
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
