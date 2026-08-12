use pwf_models::task::{CommitRanges, TaskId, TaskReport};
use pwf_wire::task::{ClosedTask, ClosedTaskAction};

#[cfg(test)]
use super::task_closure::review_task_prompt;
use super::{
    CloseTaskError,
    resolve_task_project::{self, ResolveTaskProject, ResolveTaskProjectError},
    task_closure::{self, TaskClosure},
};
use crate::ports::{
    clock::Clock,
    task_record::{IndexEntryStore, IndexSectionStore, TaskStore},
};

#[derive(Debug, Clone)]
pub struct CompleteTask {
    pub id: TaskId,
    pub report: Option<TaskReport>,
    pub commits: Option<CommitRanges>,
    pub review: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CompleteTaskError {
    #[error(transparent)]
    ResolveProject(#[from] ResolveTaskProjectError),
    #[error(transparent)]
    Close(#[from] CloseTaskError),
}

#[cqrsy::command]
pub async fn execute(
    command: &CompleteTask,
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<ClosedTask, CompleteTaskError> {
    let project = resolve_task_project::execute(
        ResolveTaskProject {
            id: command.id.clone(),
        },
        pool,
    )
    .await?;
    task_closure::close(
        &TaskClosure {
            action: ClosedTaskAction::Done,
            id: &command.id,
            completed: clock.today(),
            report: command.report.as_ref(),
            commits: command.commits.as_ref(),
            review: command.review,
        },
        store,
        &project,
    )
    .map_err(Into::into)
}

/// Reports failures shared by the done and cancel interactors.
#[cfg(test)]
mod tests {
    use pwf_models::{
        project::Project,
        task::{TaskId, TaskStatus},
    };

    use super::{
        CloseTaskError, ClosedTaskAction, CompleteTask, CompleteTaskError, review_task_prompt,
    };
    use crate::{
        ports::task_record::{IndexEntry, IndexEntryState, IndexEntryStore, TaskRecord},
        testing::{FixedClock, InMemoryStore, app_date, project, task_record},
    };

    fn record(id: &str, status: TaskStatus) -> TaskRecord {
        TaskRecord {
            status,
            ..task_record(id)
        }
    }

    fn entry(id: &str, state: IndexEntryState, section: &str) -> IndexEntry {
        IndexEntry {
            id: TaskId::try_new(id).unwrap(),
            state,
            section: (!section.is_empty()).then(|| section.parse().unwrap()),
        }
    }

    fn foo() -> Project {
        project("FOO", "foo-bar")
    }

    fn staged(tasks: Vec<TaskRecord>, entries: Vec<IndexEntry>) -> InMemoryStore {
        let store = InMemoryStore::default()
            .with_project_id("foo-bar", "FOO")
            .with_project("foo-bar", tasks);
        for entry in entries {
            IndexEntryStore::upsert_index_entry(&store, &foo(), entry).unwrap();
        }
        store
    }

    fn done_command(id: &str) -> CompleteTask {
        CompleteTask {
            id: id.parse().unwrap(),
            report: None,
            commits: None,
            review: false,
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_marks_entry_and_evicts_past_cap(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
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
                    IndexEntryState::Done(Some(app_date(format!("2026-01-{n:02}")))),
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
            IndexEntryState::Done(Some(app_date("2026-07-26")))
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
    async fn done_uses_the_clock_date(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
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
        super::execute(&done_command("FOO-0001"), &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(
            store.tasks("foo-bar")[0].completed,
            Some(app_date("2026-07-26"))
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_updates_an_unindexed_active_task(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
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
            "FOO",
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
            "FOO",
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
            commits: "a..b".parse().ok(),
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
            "FOO",
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
            CompleteTaskError::Close(CloseTaskError::TaskNotFound { ref id })
                if id.as_ref() == "FOO-9999"
        ));
        assert_eq!(error.to_string(), "Active task not found: FOO-9999");
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_rejects_an_invalid_persisted_title_before_mutation(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let invalid = TaskRecord {
            title: "x".repeat(201),
            ..record("FOO-0001", TaskStatus::Active)
        };
        let store = staged(
            vec![invalid],
            vec![entry("FOO-0001", IndexEntryState::Open, "General")],
        );

        let error = super::execute(&done_command("FOO-0001"), &store, &pool, &FixedClock)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            CompleteTaskError::Close(CloseTaskError::InvalidTitle { .. })
        ));
        assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Active);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_reports_an_unknown_project_id(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
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
            review_task_prompt(&"PWF-0128".parse().unwrap(), Some("a..b")).as_ref(),
            "review PWF-0128, commits: a..b / git-tools diff a..b / git-tools diff-subrepos"
        );
        assert_eq!(
            review_task_prompt(&"PWF-0128".parse().unwrap(), None).as_ref(),
            "review PWF-0128 / git-tools diff / git-tools diff-subrepos"
        );
    }
}
