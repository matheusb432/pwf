use pwf_models::pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus};

#[cfg(test)]
use super::logic::pending_work_closing::review_task_prompt;
use super::{
    ProjectRegistry,
    add_pending_work_item::{AddPendingWorkError, AddPendingWorkItemOk},
    logic::pending_work_closing::{CloseError, perform_close},
};
use crate::ports::{
    app_record::AppRecordStore,
    clock::Clock,
    pending_work_record::{IndexEntry, IndexSection, PendingWorkRecord},
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

    pub(in crate::pending_work) fn status(self) -> WorkItemStatus {
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
pub fn execute(
    command: &CompletePendingWork,
    store: &(
         impl AppRecordStore<PendingWorkRecord>
         + AppRecordStore<IndexEntry>
         + AppRecordStore<IndexSection>
     ),
    projects: &ProjectRegistry,
    clock: &impl Clock,
) -> Result<CompletePendingWorkOk, CompletePendingWorkError> {
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
#[cfg(test)]
mod tests {
    use pwf_models::pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus};

    use super::{
        AddPendingWorkError, ClosedItemAction, CompletePendingWork, CompletePendingWorkError,
        ProjectRegistry, review_task_prompt,
    };
    use crate::{
        ports::{
            app_record::AppRecordStore,
            clock::Clock,
            pending_work_record::{
                IndexEntry, IndexEntryState, Materialization, PendingWorkRecord, RecordId,
            },
        },
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
            <InMemoryStore as AppRecordStore<IndexEntry>>::insert(&store, &foo(), entry).unwrap();
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
