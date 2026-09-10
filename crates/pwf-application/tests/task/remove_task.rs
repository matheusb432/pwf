use pwf_application::{
    ports::{
        confirmation::{ConfirmationClient, ConfirmationClientError},
        task_vault::{IndexEntry, IndexEntryState, TaskVault},
    },
    task::{remove_task, remove_task::RemoveTaskError},
};
use pwf_models::task::{TaskId, TaskStatus};
use pwf_wire::{
    confirmation::RemoveTaskConfirmation,
    task::{
        DeleteTask, DeleteTaskOutcome, Materialization, TaskMutationResult, TaskNotePath,
        TaskRecord,
    },
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
            request_id: None,
            request_fingerprint: None,
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
    let index_state = match status {
        TaskStatus::Active => IndexEntryState::Open,
        TaskStatus::Done | TaskStatus::Cancelled => {
            IndexEntryState::Done(Some(task_timestamp("2026-07-02T12:34:56Z")))
        }
    };
    let store = InMemoryStore::default()
        .with_project_id("foo", "FOO")
        .with_project("foo", vec![record("FOO-0001", status)]);
    TaskVault::upsert_index_entry(
        &store,
        &project("FOO", "foo"),
        IndexEntry {
            id: TaskId::try_new("FOO-0001").unwrap(),
            state: index_state,
            section: None,
        },
    )
    .unwrap();
    store
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
async fn changed_vault_after_confirmation_preserves_the_task_and_index(pool: sqlx::SqlitePool) {
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
    assert_eq!(store.entries("foo").len(), 1);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn remove_deletes_record_and_index_entry(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store = staged(TaskStatus::Active);

    let outcome = run(&task_id("FOO-0001"), &store, &pool, &mut Accepted)
        .await
        .unwrap();
    assert_eq!(outcome.outcome, DeleteTaskOutcome::Deleted);
    assert!(store.tasks("foo").is_empty(), "record must be deleted");
    assert!(
        store.entries("foo").is_empty(),
        "index entry must be unlinked"
    );
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
    assert_eq!(store.entries("foo").len(), 1);
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
    assert_eq!(store.entries("foo").len(), 1);
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
async fn remove_deletes_an_unindexed_task(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store = InMemoryStore::default()
        .with_project_id("foo", "FOO")
        .with_project("foo", vec![record("FOO-0001", TaskStatus::Active)]);

    let outcome = run(&task_id("FOO-0001"), &store, &pool, &mut Accepted)
        .await
        .unwrap();

    assert_eq!(outcome.outcome, DeleteTaskOutcome::Deleted);
    assert!(store.tasks("foo").is_empty());
    assert!(store.entries("foo").is_empty());
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
        assert!(store.entries("foo").is_empty(), "{status} index retained");
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

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn remove_rejects_missing_note_wikilink_with_its_path(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let ghost = TaskRecord {
        materialization: Materialization::MissingNote {
            expected: TaskNotePath::new("/notes/foo/FOO-0001.md".into()),
        },
        ..record("FOO-0001", TaskStatus::Active)
    };
    let store = InMemoryStore::default()
        .with_project_id("foo", "FOO")
        .with_project("foo", vec![ghost]);

    let error = run(&task_id("FOO-0001"), &store, &pool, &mut Accepted)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        RemoveTaskError::NoteMissing { ref path }
            if path.as_path() == std::path::Path::new("/notes/foo/FOO-0001.md")
    ));
    assert_eq!(
        error.to_string(),
        "Task note missing: /notes/foo/FOO-0001.md"
    );
}

mod confirmed_removal {
    use pwf_wire::task::DeleteTaskOutcome;

    use super::*;

    #[sqlx::test(migrator = "crate::support::MIGRATOR")]
    async fn remove_deletes_record_and_index_entry_after_confirmation(pool: sqlx::SqlitePool) {
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
        assert!(store.entries("foo").is_empty());
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
        assert_eq!(store.tasks("foo").len(), 1);
        assert_eq!(store.entries("foo").len(), 1);
    }
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn completed_request_preserves_the_mutated_task_summary(pool: sqlx::SqlitePool) {
    use pwf_wire::task::{TaskMutationSummary, TaskRequestFingerprint, TaskRequestId};
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store = staged(TaskStatus::Cancelled);
    let command = DeleteTask {
        id: task_id("FOO-0001"),
        request_id: Some(TaskRequestId::try_new("request-1").unwrap()),
        request_fingerprint: Some(TaskRequestFingerprint::from_digest([1; 32])),
    };
    let first = remove_task::execute(&command, &store, &pool, &mut Accepted)
        .await
        .unwrap();
    let replay = remove_task::execute(&command, &store, &pool, &mut Accepted)
        .await
        .unwrap();
    assert_eq!(replay, first);
    assert_eq!(replay.outcome, DeleteTaskOutcome::Deleted);
    assert_eq!(
        replay.task,
        Some(TaskMutationSummary {
            id: command.id,
            title: "stale task".into(),
            status: TaskStatus::Cancelled
        })
    );
    assert!(store.tasks("foo").is_empty());
}
