use pwf_models::{
    project::ProjectId,
    task::{ProjectName, TaskId, TaskStatus, Timestamp},
};

#[cfg(test)]
use super::logic::task_closing::review_task_prompt;
use super::{
    add_task::{AddTaskError, AddTaskOk},
    logic::task_closing::{CloseError, CloseTaskRequest, perform_close},
    resolve_task_project::{self, ResolveTaskProject, ResolveTaskProjectError},
};
use crate::ports::{
    clock::Clock,
    task_record::{IndexEntryStore, IndexSectionStore, TaskStore},
};

#[derive(Debug, Clone)]
pub struct CompleteTask {
    pub id: TaskId,
    pub date: Option<String>,
    pub report: Option<String>,
    pub commits: Vec<String>,
    pub review: bool,
}

/// Selects the status and confirmation verb recorded by a close operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosedTaskAction {
    Done,
    Cancelled,
}

impl ClosedTaskAction {
    #[must_use]
    pub fn past_tense(self) -> &'static str {
        match self {
            Self::Done => "Done",
            Self::Cancelled => "Cancelled",
        }
    }

    pub(in crate::task) fn status(self) -> TaskStatus {
        match self {
            Self::Done => TaskStatus::Done,
            Self::Cancelled => TaskStatus::Cancelled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteTaskOk {
    pub id: TaskId,
    pub project: ProjectName,
    pub title: String,
    pub action: ClosedTaskAction,
    pub evicted_ids: Vec<TaskId>,
    pub futuro_renamed_project: Option<ProjectName>,
    pub review_task: Option<AddTaskOk>,
}

#[derive(Debug, thiserror::Error)]
pub enum CompleteTaskError {
    #[error("Active task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error("Unknown project ID `{project_id}` for task {task_id}")]
    UnknownProjectId {
        task_id: TaskId,
        project_id: ProjectId,
    },
    #[error("--report cannot be empty.")]
    EmptyReport,
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    ReviewTask(#[source] AddTaskError),
    #[error("{0}")]
    QueryProject(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[cqrsy::command]
pub async fn execute(
    command: &CompleteTask,
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<CompleteTaskOk, CompleteTaskError> {
    let resolved = resolve_task_project::execute(
        ResolveTaskProject {
            id: command.id.clone(),
        },
        pool,
    )
    .await
    .map_err(map_project_error)?;
    let authored_date = command
        .date
        .clone()
        .map_or_else(|| clock.today(), Timestamp::new);
    perform_close(
        store,
        &resolved.project,
        CloseTaskRequest {
            action: ClosedTaskAction::Done,
            id: &command.id,
            completed: authored_date,
            report: command.report.as_deref(),
            commits: &command.commits,
            review: command.review,
        },
    )
    .map_err(CloseError::into_complete)
}

fn map_project_error(error: ResolveTaskProjectError) -> CompleteTaskError {
    match error {
        ResolveTaskProjectError::UnknownProjectId {
            task_id,
            project_id,
        } => CompleteTaskError::UnknownProjectId {
            task_id,
            project_id,
        },
        ResolveTaskProjectError::QueryProject(source) => CompleteTaskError::QueryProject(source),
    }
}

/// Reports failures shared by the done and cancel operations.
#[cfg(test)]
mod tests {
    use pwf_models::{
        project::Project,
        task::{TaskId, TaskStatus, Timestamp},
    };

    use super::{ClosedTaskAction, CompleteTask, CompleteTaskError, review_task_prompt};
    use crate::{
        ports::{
            clock::Clock,
            task_record::{
                IndexEntry, IndexEntryState, IndexEntryStore, Materialization, TaskRecord,
            },
        },
        testing::{InMemoryStore, project},
    };

    #[derive(Clone)]
    struct FixedClock;

    impl Clock for FixedClock {
        fn today(&self) -> Timestamp {
            Timestamp::new("2026-07-26")
        }
    }

    fn record(id: &str, status: TaskStatus) -> TaskRecord {
        TaskRecord {
            id: TaskId::try_new(id).unwrap(),
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
            id: TaskId::try_new(id).unwrap(),
            state,
            section: section.to_string(),
        }
    }

    fn foo() -> Project {
        project("FOO".parse().unwrap(), "foo-bar")
    }

    fn staged(tasks: Vec<TaskRecord>, entries: Vec<IndexEntry>) -> InMemoryStore {
        let store = InMemoryStore::default()
            .with_project_id("foo-bar", "FOO".parse().unwrap())
            .with_project("foo-bar", tasks);
        for entry in entries {
            IndexEntryStore::upsert_index_entry(&store, &foo(), entry).unwrap();
        }
        store
    }

    fn done_command(id: &str) -> CompleteTask {
        CompleteTask {
            id: id.parse().unwrap(),
            date: Some("2026-07-07".to_string()),
            report: None,
            commits: Vec::new(),
            review: false,
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_marks_entry_and_evicts_past_cap(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO".parse().unwrap(),
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let mut tasks = vec![record("FOO-0007", TaskStatus::Active)];
        let mut entries: Vec<IndexEntry> = (1..=6)
            .map(|n| {
                tasks.push(record(&format!("FOO-{n:04}"), TaskStatus::Done));
                entry(
                    &format!("FOO-{n:04}"),
                    IndexEntryState::Done(Timestamp::new(format!("2026-01-{n:02}"))),
                    "General",
                )
            })
            .collect();
        entries.push(entry("FOO-0007", IndexEntryState::Open, "General"));
        let store = staged(tasks, entries);

        let out = super::execute(&done_command("FOO-0007"), &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(out.action, ClosedTaskAction::Done);
        assert_eq!(out.evicted_ids, vec![TaskId::try_new("FOO-0001").unwrap()]);
        assert_eq!(out.futuro_renamed_project, None);
        assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Done);
        let marked = store
            .entries("foo-bar")
            .into_iter()
            .find(|e| e.id == TaskId::try_new("FOO-0007").unwrap())
            .unwrap();
        assert_eq!(
            marked.state,
            IndexEntryState::Done(Timestamp::new("2026-07-07"))
        );
        assert!(
            !store
                .entries("foo-bar")
                .iter()
                .any(|e| e.id == TaskId::try_new("FOO-0001").unwrap()),
            "evicted entry must be unlinked"
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_uses_clock_date_when_no_date_is_explicit(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO".parse().unwrap(),
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(
            vec![record("FOO-0001", TaskStatus::Active)],
            vec![entry("FOO-0001", IndexEntryState::Open, "General")],
        );
        let mut command = done_command("FOO-0001");
        command.date = None;

        super::execute(&command, &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(
            store.tasks("foo-bar")[0].completed,
            Some(Timestamp::new("2026-07-26"))
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_updates_an_unindexed_active_task(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO".parse().unwrap(),
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(vec![record("FOO-0001", TaskStatus::Active)], Vec::new());

        let outcome = super::execute(&done_command("FOO-0001"), &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(outcome.action, ClosedTaskAction::Done);
        assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Done);
        assert!(store.entries("foo-bar").is_empty());
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_normalizes_futuro_header_entries(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO".parse().unwrap(),
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(
            vec![record("FOO-0001", TaskStatus::Active)],
            vec![entry("FOO-0001", IndexEntryState::Open, "Futuro")],
        );

        let out = super::execute(&done_command("FOO-0001"), &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(out.futuro_renamed_project, Some(foo().title));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_review_inserts_review_task_and_open_entry(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO".parse().unwrap(),
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(
            vec![record("FOO-0001", TaskStatus::Active)],
            vec![entry("FOO-0001", IndexEntryState::Open, "General")],
        );
        let cmd = CompleteTask {
            review: true,
            commits: vec!["a..b".to_string()],
            ..done_command("FOO-0001")
        };

        let out = super::execute(&cmd, &store, &pool, &FixedClock)
            .await
            .unwrap();

        let review = out.review_task.expect("review task present");
        assert_eq!(review.id.as_ref(), "FOO-0002");
        assert!(
            store
                .entries("foo-bar")
                .iter()
                .any(|e| e.id == TaskId::try_new("FOO-0002").unwrap()
                    && e.state == IndexEntryState::Open),
            "review task must get an open index entry"
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_on_missing_item_reports_item_not_found(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO".parse().unwrap(),
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(Vec::new(), Vec::new());

        let error = super::execute(&done_command("FOO-9999"), &store, &pool, &FixedClock)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            CompleteTaskError::TaskNotFound { ref id } if id.as_ref() == "FOO-9999"
        ));
        assert_eq!(error.to_string(), "Active task not found: FOO-9999");
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_reports_an_unknown_project_id(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO".parse().unwrap(),
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(Vec::new(), Vec::new());

        let error = super::execute(&done_command("XYZ-0001"), &store, &pool, &FixedClock)
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown project ID `XYZ` for task XYZ-0001"
        );
    }

    #[test]
    fn review_prompt_uses_scoped_or_bare_diff() {
        assert_eq!(
            review_task_prompt(&"PWF-0128".parse().unwrap(), Some("a..b")),
            "review PWF-0128, commits: a..b / git-tools diff a..b / git-tools diff-subrepos"
        );
        assert_eq!(
            review_task_prompt(&"PWF-0128".parse().unwrap(), None),
            "review PWF-0128 / git-tools diff / git-tools diff-subrepos"
        );
    }
}
