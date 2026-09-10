use pwf_application::task::cancel_task;
use pwf_models::task::TaskStatus;
use pwf_wire::task::{CancelTask, TaskRecord};

use crate::support::{
    FixedClock, InMemoryStore, InMemoryStoreFailure, task_record, task_timestamp,
};

fn record(id: &str) -> TaskRecord {
    task_record(id)
}

fn staged() -> InMemoryStore {
    InMemoryStore::default()
        .with_project_id("foo-bar", "FOO")
        .with_project("foo-bar", vec![record("FOO-0001")])
        .with_failure(InMemoryStoreFailure::ListTasks)
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn cancel_marks_item_cancelled(pool: sqlx::SqlitePool) {
    crate::support::insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let store = staged();
    let command = CancelTask {
        id: "FOO-0001".parse().unwrap(),
        report: "obsoleted".parse().unwrap(),
        commits: "a..b, c..d".parse().ok(),
        expected_revision: None,
    };

    cancel_task::execute(&command, &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Cancelled);
    assert_eq!(
        store.tasks("foo-bar")[0].completed_at,
        Some(task_timestamp("2026-07-26T12:34:56Z"))
    );
    assert_eq!(
        store.tasks("foo-bar")[0].commits.as_deref(),
        Some("a..b, c..d")
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn cancel_uses_the_clock_timestamp(pool: sqlx::SqlitePool) {
    crate::support::insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let store = staged();
    let command = CancelTask {
        id: "FOO-0001".parse().unwrap(),
        report: "obsoleted".parse().unwrap(),
        commits: None,
        expected_revision: None,
    };

    cancel_task::execute(&command, &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(
        store.tasks("foo-bar")[0].completed_at,
        Some(task_timestamp("2026-07-26T12:34:56Z"))
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn cancel_reports_an_unknown_project_id(pool: sqlx::SqlitePool) {
    crate::support::insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let command = CancelTask {
        id: "XYZ-0001".parse().unwrap(),
        report: "obsolete".parse().unwrap(),
        commits: None,
        expected_revision: None,
    };

    let error = cancel_task::execute(&command, &staged(), &pool, &FixedClock)
        .await
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "Unknown project ID `XYZ` for task XYZ-0001"
    );
}
