use pwf_application::{
    ports::confirmation::{ConfirmationClient, ConfirmationClientError},
    task::{remove_task, remove_task::RemoveTaskError},
};
use pwf_models::task::{TaskId, TaskStatus};
use pwf_wire::{
    confirmation::RemoveTaskConfirmation,
    task::{DeleteTask, DeleteTaskOutcome, TaskMutationResult, TaskNotePath, TaskRecord},
};

use crate::support::{
    InMemoryStore, insert_project, project, stored_blocked_by, task_record, task_timestamp,
};

async fn run(
    task_id: &TaskId,
    store: &InMemoryStore,
    pool: &sqlx::SqlitePool,
    confirmation: &mut dyn ConfirmationClient<Confirmation = RemoveTaskConfirmation>,
) -> Result<TaskMutationResult<DeleteTaskOutcome>, RemoveTaskError> {
    remove_task::execute(
        &DeleteTask {
            id: task_id.clone(),
        },
        store,
        pool,
        confirmation,
    )
    .await
}

fn record(id: &str, status: TaskStatus) -> TaskRecord {
    TaskRecord {
        title: "stale task".to_string(),
        status,
        created_at: Some(task_timestamp("2026-07-01T12:34:56Z")),
        locator: TaskNotePath::new(format!("/notes/foo/{id}.md").into()),
        ..task_record(id)
    }
}

fn staged(status: TaskStatus) -> InMemoryStore {
    InMemoryStore::default()
        .with_project_id("foo", "FOO")
        .with_project("foo", vec![record("FOO-0001", status)])
}

fn task_id(id: &str) -> TaskId {
    id.parse().unwrap()
}

struct Accepted;

impl ConfirmationClient for Accepted {
    type Confirmation = RemoveTaskConfirmation;

    fn confirm<'a>(
        &'a mut self,
        _confirmation: &'a RemoveTaskConfirmation,
    ) -> futures::future::BoxFuture<'a, Result<bool, ConfirmationClientError>> {
        Box::pin(futures::future::ready(Ok(true)))
    }
}

struct EditThenAccept {
    store: InMemoryStore,
    project: pwf_models::project::Project,
    id: TaskId,
}

impl ConfirmationClient for EditThenAccept {
    type Confirmation = RemoveTaskConfirmation;

    fn confirm<'a>(
        &'a mut self,
        _confirmation: &'a RemoveTaskConfirmation,
    ) -> futures::future::BoxFuture<'a, Result<bool, ConfirmationClientError>> {
        self.store.externally_edit_task(&self.project, &self.id);
        Box::pin(futures::future::ready(Ok(true)))
    }
}

struct StaticInteraction {
    accepted: bool,
}

impl ConfirmationClient for StaticInteraction {
    type Confirmation = RemoveTaskConfirmation;

    fn confirm<'a>(
        &'a mut self,
        _confirmation: &'a RemoveTaskConfirmation,
    ) -> futures::future::BoxFuture<'a, Result<bool, ConfirmationClientError>> {
        Box::pin(futures::future::ready(Ok(self.accepted)))
    }
}

struct ChangeVaultThenAccept {
    pool: sqlx::SqlitePool,
}

impl ConfirmationClient for ChangeVaultThenAccept {
    type Confirmation = RemoveTaskConfirmation;

    fn confirm<'a>(
        &'a mut self,
        confirmation: &'a RemoveTaskConfirmation,
    ) -> futures::future::BoxFuture<'a, Result<bool, ConfirmationClientError>> {
        assert_eq!(
            confirmation.deletion,
            pwf_wire::confirmation::TaskDeletion::HardDelete
        );
        Box::pin(async move {
            sqlx::query("UPDATE projects SET obsidian_vault = '/different/vault' WHERE id = 'FOO'")
                .execute(&self.pool)
                .await
                .unwrap();
            Ok(true)
        })
    }
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn changed_vault_after_confirmation_preserves_the_task(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store = staged(TaskStatus::Active);
    let before = store.tasks("foo");
    let error = run(
        &task_id("FOO-0001"),
        &store,
        &pool,
        &mut ChangeVaultThenAccept { pool: pool.clone() },
    )
    .await
    .unwrap_err();
    assert!(matches!(error, RemoveTaskError::DeletionChanged));
    assert_eq!(store.tasks("foo"), before);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn remove_deletes_the_task_note(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store = staged(TaskStatus::Active);

    let outcome = run(&task_id("FOO-0001"), &store, &pool, &mut Accepted)
        .await
        .unwrap();
    assert_eq!(outcome.outcome, DeleteTaskOutcome::Deleted);
    assert!(store.tasks("foo").is_empty(), "record must be deleted");
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn remove_rejects_a_task_edited_after_preflight_without_unlinking(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store = staged(TaskStatus::Active);
    let id = task_id("FOO-0001");
    let mut confirmation = EditThenAccept {
        store: store.clone(),
        project: project("FOO", "foo"),
        id: id.clone(),
    };

    let error = run(&id, &store, &pool, &mut confirmation)
        .await
        .unwrap_err();

    assert!(matches!(error, RemoveTaskError::Revision(_)));
    assert_eq!(store.tasks("foo").len(), 1);
    assert!(store.tasks("foo")[0].source.ends_with("external edit\n"));
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn remove_reports_all_dependents_including_paused_projects_without_mutating(
    pool: sqlx::SqlitePool,
) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    insert_project(
        &pool,
        "AUX",
        "paused-project",
        "/projects/paused",
        "/tasks/paused",
        true,
    )
    .await;
    let local_dependent = TaskRecord {
        blocked_by: stored_blocked_by(&["FOO-0001"]),
        ..record("FOO-0003", TaskStatus::Done)
    };
    let paused_dependent = TaskRecord {
        blocked_by: stored_blocked_by(&["FOO-0001"]),
        ..record("AUX-0002", TaskStatus::Active)
    };
    let store = staged(TaskStatus::Active)
        .with_project(
            "foo",
            vec![record("FOO-0001", TaskStatus::Active), local_dependent],
        )
        .with_project("paused-project", vec![paused_dependent]);
    let before = store.tasks("foo");

    let error = run(&task_id("FOO-0001"), &store, &pool, &mut Accepted)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        RemoveTaskError::HasDependents { ref target, ref dependents }
            if target.as_ref() == "FOO-0001"
                && dependents.iter().map(AsRef::as_ref).collect::<Vec<_>>()
                    == ["AUX-0002", "FOO-0003"]
    ));
    assert_eq!(store.tasks("foo"), before);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn remove_rejects_an_invalid_persisted_title_before_mutation(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store = InMemoryStore::default()
        .with_project_id("foo", "FOO")
        .with_project(
            "foo",
            vec![TaskRecord {
                title: "x".repeat(201),
                ..record("FOO-0001", TaskStatus::Active)
            }],
        );

    let error = run(&task_id("FOO-0001"), &store, &pool, &mut Accepted)
        .await
        .unwrap_err();

    assert!(matches!(error, RemoveTaskError::InvalidTitle { .. }));
    assert_eq!(store.tasks("foo").len(), 1);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn remove_deletes_closed_items(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    for status in [TaskStatus::Done, TaskStatus::Cancelled] {
        let store = staged(status);

        let outcome = run(&task_id("FOO-0001"), &store, &pool, &mut Accepted)
            .await
            .unwrap();

        assert_eq!(outcome.outcome, DeleteTaskOutcome::Deleted);
        assert!(store.tasks("foo").is_empty(), "{status} record retained");
    }
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn remove_missing_item_preserves_requested_id(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store = staged(TaskStatus::Active);

    let error = run(&task_id("FOO-9999"), &store, &pool, &mut Accepted)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        RemoveTaskError::TaskNotFound { ref id } if id.as_ref() == "FOO-9999"
    ));
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn remove_reports_an_unknown_project_id(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store = staged(TaskStatus::Active);

    let error = run(&task_id("XYZ-0001"), &store, &pool, &mut Accepted)
        .await
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "Unknown project ID `XYZ` for task XYZ-0001"
    );
}

mod confirmed_removal {
    use pwf_wire::task::DeleteTaskOutcome;

    use super::*;

    #[sqlx::test(migrator = "crate::support::MIGRATOR")]
    async fn remove_deletes_the_task_note_after_confirmation(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = staged(TaskStatus::Active);

        let outcome = run(
            &task_id("FOO-0001"),
            &store,
            &pool,
            &mut StaticInteraction { accepted: true },
        )
        .await
        .unwrap();
        assert_eq!(outcome.outcome, DeleteTaskOutcome::Deleted);
        assert!(store.tasks("foo").is_empty());
    }

    #[sqlx::test(migrator = "crate::support::MIGRATOR")]
    async fn remove_decline_returns_aborted_without_mutating_task(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = staged(TaskStatus::Active);

        let outcome = run(
            &task_id("FOO-0001"),
            &store,
            &pool,
            &mut StaticInteraction { accepted: false },
        )
        .await
        .unwrap();

        assert_eq!(outcome.outcome, DeleteTaskOutcome::Aborted);
        assert!(outcome.task.is_none());
        assert_eq!(store.tasks("foo").len(), 1);
    }
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn removal_returns_the_task_summary_and_reports_missing_on_repeat(pool: sqlx::SqlitePool) {
    use pwf_wire::task::TaskMutationSummary;
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store = staged(TaskStatus::Cancelled);
    let command = DeleteTask {
        id: task_id("FOO-0001"),
    };
    let removed = remove_task::execute(&command, &store, &pool, &mut Accepted)
        .await
        .unwrap();
    assert_eq!(removed.outcome, DeleteTaskOutcome::Deleted);
    assert_eq!(
        removed.task,
        Some(TaskMutationSummary {
            id: command.id.clone(),
            title: "stale task".into(),
            status: TaskStatus::Cancelled,
        })
    );
    assert!(store.tasks("foo").is_empty());

    let error = remove_task::execute(&command, &store, &pool, &mut Accepted)
        .await
        .unwrap_err();
    assert!(matches!(error, RemoveTaskError::TaskNotFound { id } if id == command.id));
}
