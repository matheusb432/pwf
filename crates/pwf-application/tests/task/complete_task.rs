use pwf_application::task::{CloseTaskError, complete_task, complete_task::CompleteTaskError};
use pwf_models::task::TaskStatus;
use pwf_wire::task::{CompleteTask, TaskRecord};

use crate::support::{
    FixedClock, InMemoryStore, InMemoryStoreFailure, task_record, task_timestamp,
};

fn record(id: &str, status: TaskStatus) -> TaskRecord {
    TaskRecord {
        status,
        ..task_record(id)
    }
}

fn staged(tasks: Vec<TaskRecord>) -> InMemoryStore {
    InMemoryStore::default()
        .with_project_id("foo-bar", "FOO")
        .with_project("foo-bar", tasks)
}

fn done_command(id: &str) -> CompleteTask {
    CompleteTask {
        id: id.parse().unwrap(),
        report: None,
        commits: None,
        expected_revision: None,
    }
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn done_changes_only_the_target_without_listing_other_notes(pool: sqlx::SqlitePool) {
    crate::support::insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let previous: Vec<_> = (1..=7)
        .map(|number| TaskRecord {
            completed_at: Some(task_timestamp("2026-01-01T00:00:00Z")),
            ..record(&format!("FOO-{number:04}"), TaskStatus::Done)
        })
        .collect();
    let mut tasks = previous.clone();
    tasks.push(record("FOO-0008", TaskStatus::Active));
    let store = staged(tasks).with_failure(InMemoryStoreFailure::ListTasks);

    let result = complete_task::execute(&done_command("FOO-0008"), &store, &pool, &FixedClock)
        .await
        .unwrap();

    let tasks = store.tasks("foo-bar");
    assert_eq!(tasks.len(), 8);
    assert_eq!(&tasks[..7], previous.as_slice());
    assert_eq!(tasks[7].status, TaskStatus::Done);
    assert_eq!(
        tasks[7].completed_at,
        Some(task_timestamp("2026-07-26T09:34:56-03:00"))
    );
    assert_eq!(result.task.unwrap().id.as_ref(), "FOO-0008");
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn done_uses_the_clock_timestamp(pool: sqlx::SqlitePool) {
    crate::support::insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let store = staged(vec![record("FOO-0001", TaskStatus::Active)]);
    complete_task::execute(&done_command("FOO-0001"), &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(
        store.tasks("foo-bar")[0].completed_at,
        Some(task_timestamp("2026-07-26T09:34:56-03:00"))
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn done_rejects_a_stale_note_revision_without_mutating(pool: sqlx::SqlitePool) {
    crate::support::insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let store = staged(vec![record("FOO-0001", TaskStatus::Active)]);
    let mut command = done_command("FOO-0001");
    command.expected_revision = Some(store.tasks("foo-bar")[0].revision.clone());
    store.externally_edit_task(&crate::support::project("FOO", "foo-bar"), &command.id);
    let before = store.tasks("foo-bar");

    let error = complete_task::execute(&command, &store, &pool, &FixedClock)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        CompleteTaskError::Close(CloseTaskError::Revision(_))
    ));
    assert_eq!(store.tasks("foo-bar"), before);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn done_on_missing_item_reports_item_not_found(pool: sqlx::SqlitePool) {
    crate::support::insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let store = staged(Vec::new());

    let error = complete_task::execute(&done_command("FOO-9999"), &store, &pool, &FixedClock)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        CompleteTaskError::Close(CloseTaskError::TaskNotFound { ref id })
            if id.as_ref() == "FOO-9999"
    ));
    assert_eq!(error.to_string(), "Active task not found: FOO-9999");
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn done_does_not_validate_an_unreturned_persisted_title(pool: sqlx::SqlitePool) {
    crate::support::insert_project(
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
    let store = staged(vec![invalid]);

    complete_task::execute(&done_command("FOO-0001"), &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Done);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn done_reports_an_unknown_project_id(pool: sqlx::SqlitePool) {
    crate::support::insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let store = staged(Vec::new());

    let error = complete_task::execute(&done_command("XYZ-0001"), &store, &pool, &FixedClock)
        .await
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "Unknown project ID `XYZ` for task XYZ-0001"
    );
}
