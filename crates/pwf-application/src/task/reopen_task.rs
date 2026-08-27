use pwf_models::task::{TaskId, TaskStatus};
use pwf_wire::{confirmation::ReopenTaskConfirmation, task::ReopenedTask};

use super::{
    note_body::remove_report,
    resolve_task_project::{self, ResolveTaskProjectError},
    task_body_region,
};
use crate::ports::{
    confirmation::{ConfirmationClient, ConfirmationClientError},
    task_record::{
        IndexEntry, IndexEntryState, IndexEntryStore, NullablePatch, TaskPatch, TaskStore,
    },
};

#[derive(Debug, thiserror::Error)]
pub enum ReopenTaskError {
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error(transparent)]
    ResolveProject(#[from] ResolveTaskProjectError),
    #[error(transparent)]
    Confirmation(#[from] ConfirmationClientError),
    #[error(transparent)]
    WriteStore(anyhow::Error),
}

/// Reopens a closed task and restores an existing queue link.
///
/// The update clears completion metadata and its appended report. An active task returns an
/// idempotent skip.
#[cqrsy::command]
pub async fn execute(
    task_id: &TaskId,
    store: &(impl TaskStore + IndexEntryStore),
    pool: &sqlx::SqlitePool,
    confirmation_client: &mut dyn ConfirmationClient<Confirmation = ReopenTaskConfirmation>,
) -> Result<ReopenedTask, ReopenTaskError> {
    let project = resolve_task_project::execute(task_id.clone(), pool).await?;
    let task_identifier = task_id.clone();
    let record = TaskStore::get(store, &project, &task_identifier)
        .map_err(|error| ReopenTaskError::WriteStore(anyhow::Error::new(error)))?
        .ok_or_else(|| ReopenTaskError::TaskNotFound {
            id: task_identifier.clone(),
        })?;

    if record.status == TaskStatus::Active {
        return Ok(ReopenedTask::AlreadyActive {
            id: task_identifier,
            project: project.title.clone(),
        });
    }

    let (body_without_report, report) = remove_report(task_body_region(&record.body));
    let confirmation = ReopenTaskConfirmation {
        task_identifier: task_identifier.clone(),
        project: project.title.clone(),
        completion_date: record.completed,
        commit_provenance: record.commits.clone(),
        report: report.clone(),
    };
    if !confirmation_client.confirm(&confirmation).await? {
        return Ok(ReopenedTask::Aborted {
            id: task_identifier,
        });
    }

    TaskStore::update(
        store,
        &project,
        &task_identifier,
        TaskPatch {
            status: Some(TaskStatus::Active),
            completed: NullablePatch::Clear,
            commits: NullablePatch::Clear,
            body: report.is_some().then_some(body_without_report),
            ..TaskPatch::default()
        },
    )
    .map_err(|error| ReopenTaskError::WriteStore(anyhow::Error::new(error)))?;

    let entries = IndexEntryStore::list_index_entries(store, &project)
        .map_err(|error| ReopenTaskError::WriteStore(anyhow::Error::new(error)))?;
    if entries
        .iter()
        .any(|entry| entry.id == task_identifier && matches!(entry.state, IndexEntryState::Done(_)))
    {
        IndexEntryStore::upsert_index_entry(
            store,
            &project,
            IndexEntry {
                id: task_identifier.clone(),
                state: IndexEntryState::Open,
                section: None,
            },
        )
        .map_err(|error| ReopenTaskError::WriteStore(anyhow::Error::new(error)))?;
    }

    Ok(ReopenedTask::Reopened {
        id: task_identifier,
        project: project.title.clone(),
    })
}

#[cfg(test)]
mod tests {
    use pwf_models::{
        project::{Project, ProjectName},
        task::{TaskId, TaskStatus},
    };
    use pwf_wire::{confirmation::ReopenTaskConfirmation, task::ReopenedTask};

    use crate::{
        ports::{
            confirmation::{ConfirmationClient, ConfirmationClientError},
            task_record::{IndexEntry, IndexEntryState, IndexEntryStore, TaskRecord},
        },
        task::reopen_task,
        testing::{InMemoryStore, app_date, project, task_record},
    };

    struct TestConfirmation {
        accepted: bool,
        recorded: Vec<ReopenTaskConfirmation>,
    }

    impl TestConfirmation {
        fn accepting() -> Self {
            Self {
                accepted: true,
                recorded: Vec::new(),
            }
        }

        fn declining() -> Self {
            Self {
                accepted: false,
                recorded: Vec::new(),
            }
        }

        fn recorded(&self) -> &[ReopenTaskConfirmation] {
            &self.recorded
        }
    }

    impl ConfirmationClient for TestConfirmation {
        type Confirmation = ReopenTaskConfirmation;

        fn confirm<'a>(
            &'a mut self,
            confirmation: &'a ReopenTaskConfirmation,
        ) -> futures::future::BoxFuture<'a, Result<bool, ConfirmationClientError>> {
            Box::pin(async move {
                self.recorded.push(confirmation.clone());
                Ok(self.accepted)
            })
        }
    }

    fn record(id: &str, status: TaskStatus) -> TaskRecord {
        TaskRecord {
            status,
            completed: (status != TaskStatus::Active).then(|| app_date("2026-01-02")),
            commits: Some("a..b".to_string()),
            body: if status == TaskStatus::Active {
                "## Goals\n\n- ship the work\n".to_string()
            } else {
                "## Goals\n\n- ship the work\n\n### Report\n\ncompleted safely\n".to_string()
            },
            ..task_record(id)
        }
    }

    fn foo() -> Project {
        project("FOO", "foo-bar")
    }

    fn staged(status: TaskStatus, entries: Vec<IndexEntry>) -> InMemoryStore {
        let store = InMemoryStore::default()
            .with_project_id("foo-bar", "FOO")
            .with_project("foo-bar", vec![record("FOO-0001", status)]);
        for entry in entries {
            IndexEntryStore::upsert_index_entry(&store, &foo(), entry).unwrap();
        }
        store
    }

    fn entry(state: IndexEntryState) -> IndexEntry {
        IndexEntry {
            id: TaskId::try_new("FOO-0001").unwrap(),
            state,
            section: None,
        }
    }

    fn task_id() -> TaskId {
        "FOO-0001".parse().unwrap()
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn reopen_restores_done_entry(pool: sqlx::SqlitePool) {
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
            TaskStatus::Done,
            vec![entry(IndexEntryState::Done(Some(app_date("2026-01-02"))))],
        );
        let mut confirmation = TestConfirmation::accepting();

        let out = reopen_task::execute(&task_id(), &store, &pool, &mut confirmation)
            .await
            .unwrap();

        assert!(matches!(out, ReopenedTask::Reopened { .. }));
        assert_eq!(
            confirmation.recorded(),
            vec![ReopenTaskConfirmation {
                task_identifier: "FOO-0001".parse().unwrap(),
                project: ProjectName::try_new("foo-bar").unwrap(),
                completion_date: Some(app_date("2026-01-02")),
                commit_provenance: Some("a..b".to_string()),
                report: Some("completed safely".to_string()),
            }]
        );
        assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Active);
        assert_eq!(store.tasks("foo-bar")[0].completed, None);
        assert_eq!(store.tasks("foo-bar")[0].commits, None);
        assert_eq!(
            store.tasks("foo-bar")[0].body,
            "## Goals\n\n- ship the work"
        );
        assert_eq!(store.entries("foo-bar")[0].state, IndexEntryState::Open);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn reopen_preserves_absent_index_entry(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(TaskStatus::Done, Vec::new());
        let mut confirmation = TestConfirmation::accepting();

        let out = reopen_task::execute(&task_id(), &store, &pool, &mut confirmation)
            .await
            .unwrap();

        assert!(matches!(out, ReopenedTask::Reopened { .. }));
        assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Active);
        assert!(store.entries("foo-bar").is_empty());
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn reopen_already_active_is_idempotent_skip(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(TaskStatus::Active, vec![entry(IndexEntryState::Open)]);
        let mut confirmation = TestConfirmation::accepting();

        let out = reopen_task::execute(&task_id(), &store, &pool, &mut confirmation)
            .await
            .unwrap();

        assert!(matches!(out, ReopenedTask::AlreadyActive { .. }));
        assert!(confirmation.recorded().is_empty());
        assert_eq!(store.tasks("foo-bar")[0].commits.as_deref(), Some("a..b"));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn declined_reopen_preserves_every_completion_artifact(pool: sqlx::SqlitePool) {
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
            TaskStatus::Done,
            vec![entry(IndexEntryState::Done(Some(app_date("2026-01-02"))))],
        );
        let tasks_before = store.tasks("foo-bar");
        let entries_before = store.entries("foo-bar");
        let mut confirmation = TestConfirmation::declining();

        let outcome = reopen_task::execute(&task_id(), &store, &pool, &mut confirmation)
            .await
            .unwrap();

        assert_eq!(
            outcome,
            ReopenedTask::Aborted {
                id: "FOO-0001".parse().unwrap(),
            }
        );
        assert_eq!(store.tasks("foo-bar"), tasks_before);
        assert_eq!(store.entries("foo-bar"), entries_before);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn reopen_reports_an_unknown_project_id(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(TaskStatus::Done, Vec::new());
        let task_id = "XYZ-0001".parse().unwrap();
        let mut confirmation = TestConfirmation::accepting();

        let error = reopen_task::execute(&task_id, &store, &pool, &mut confirmation)
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown project ID `XYZ` for task XYZ-0001"
        );
    }
}
