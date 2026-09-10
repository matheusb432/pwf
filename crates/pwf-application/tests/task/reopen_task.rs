use pwf_application::{
    ports::confirmation::{ConfirmationClient, ConfirmationClientError},
    task::reopen_task,
};
use pwf_models::{
    project::Project,
    task::{TaskId, TaskStatus},
};
use pwf_wire::{
    confirmation::ReopenTaskConfirmation,
    task::{ReopenTask, ReopenTaskOutcome, TaskRecord},
};

use crate::support::{InMemoryStore, app_date, project, task_record, task_timestamp};

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
        self.recorded.push(confirmation.clone());
        Box::pin(futures::future::ready(Ok(self.accepted)))
    }
}

struct EditThenAccept {
    store: InMemoryStore,
    project: Project,
    id: TaskId,
}

impl ConfirmationClient for EditThenAccept {
    type Confirmation = ReopenTaskConfirmation;

    fn confirm<'a>(
        &'a mut self,
        _confirmation: &'a ReopenTaskConfirmation,
    ) -> futures::future::BoxFuture<'a, Result<bool, ConfirmationClientError>> {
        self.store.externally_edit_task(&self.project, &self.id);
        Box::pin(futures::future::ready(Ok(true)))
    }
}

fn record(id: &str, status: TaskStatus) -> TaskRecord {
    TaskRecord {
        status,
        completed_at: (status != TaskStatus::Active)
            .then(|| task_timestamp("2026-01-02T12:34:56Z")),
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

fn staged(status: TaskStatus) -> InMemoryStore {
    InMemoryStore::default()
        .with_project_id("foo-bar", "FOO")
        .with_project("foo-bar", vec![record("FOO-0001", status)])
}

fn command(id: &str) -> ReopenTask {
    ReopenTask {
        id: id.parse().unwrap(),
    }
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn reopen_clears_the_note_completion_metadata(pool: sqlx::SqlitePool) {
    crate::support::insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let store = staged(TaskStatus::Done);
    let mut confirmation = TestConfirmation::accepting();

    let out = reopen_task::execute(&command("FOO-0001"), &store, &pool, &mut confirmation)
        .await
        .unwrap();

    assert_eq!(out.outcome, ReopenTaskOutcome::Reopened);
    let confirmations = confirmation.recorded();
    assert_eq!(confirmations.len(), 1);
    let Some(confirmation) = confirmations.first() else {
        return;
    };
    assert_eq!(confirmation.task_identifier.as_ref(), "FOO-0001");
    assert_eq!(confirmation.project.as_ref(), "foo-bar");
    assert_eq!(confirmation.completion_date, Some(app_date("2026-01-02")));
    assert_eq!(confirmation.commit_provenance.as_deref(), Some("a..b"));
    assert_eq!(confirmation.report.as_deref(), Some("completed safely"));
    assert_eq!(confirmation.revision.as_ref().len(), 64);
    assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Active);
    assert_eq!(store.tasks("foo-bar")[0].completed_at, None);
    assert_eq!(store.tasks("foo-bar")[0].commits, None);
    assert_eq!(
        store.tasks("foo-bar")[0].body,
        "## Goals\n\n- ship the work"
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn reopen_rejects_a_task_edited_after_preflight_without_mutation(pool: sqlx::SqlitePool) {
    crate::support::insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let store = staged(TaskStatus::Done);
    let mut confirmation = EditThenAccept {
        store: store.clone(),
        project: foo(),
        id: TaskId::try_new("FOO-0001").unwrap(),
    };

    let error = reopen_task::execute(&command("FOO-0001"), &store, &pool, &mut confirmation)
        .await
        .unwrap_err();

    assert!(matches!(error, reopen_task::ReopenTaskError::Revision(_)));
    assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Done);
    assert!(
        store.tasks("foo-bar")[0]
            .source
            .ends_with("external edit\n")
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn reopen_accepts_a_cancelled_task_note(pool: sqlx::SqlitePool) {
    crate::support::insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let store = staged(TaskStatus::Cancelled);
    let mut confirmation = TestConfirmation::accepting();

    let out = reopen_task::execute(&command("FOO-0001"), &store, &pool, &mut confirmation)
        .await
        .unwrap();

    assert_eq!(out.outcome, ReopenTaskOutcome::Reopened);
    assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Active);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn reopen_already_active_is_idempotent_skip(pool: sqlx::SqlitePool) {
    crate::support::insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let store = staged(TaskStatus::Active);
    let mut confirmation = TestConfirmation::accepting();

    let out = reopen_task::execute(&command("FOO-0001"), &store, &pool, &mut confirmation)
        .await
        .unwrap();

    assert_eq!(out.outcome, ReopenTaskOutcome::AlreadyActive);
    assert!(confirmation.recorded().is_empty());
    assert_eq!(store.tasks("foo-bar")[0].commits.as_deref(), Some("a..b"));
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn declined_reopen_preserves_every_completion_artifact(pool: sqlx::SqlitePool) {
    crate::support::insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let store = staged(TaskStatus::Done);
    let tasks_before = store.tasks("foo-bar");
    let mut confirmation = TestConfirmation::declining();

    let outcome = reopen_task::execute(&command("FOO-0001"), &store, &pool, &mut confirmation)
        .await
        .unwrap();

    assert_eq!(outcome.outcome, ReopenTaskOutcome::Aborted);
    assert!(outcome.task.is_none());
    assert_eq!(store.tasks("foo-bar"), tasks_before);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn reopen_reports_an_unknown_project_id(pool: sqlx::SqlitePool) {
    crate::support::insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let store = staged(TaskStatus::Done);
    let mut confirmation = TestConfirmation::accepting();

    let error = reopen_task::execute(&command("XYZ-0001"), &store, &pool, &mut confirmation)
        .await
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "Unknown project ID `XYZ` for task XYZ-0001"
    );
}
