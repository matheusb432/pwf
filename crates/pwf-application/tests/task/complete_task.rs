use pwf_application::{
    ports::task_vault::{IndexEntry, IndexEntryState, TaskVault},
    task::{CloseTaskError, complete_task, complete_task::CompleteTaskError},
};
use pwf_models::{
    project::Project,
    task::{TaskId, TaskStatus},
};
use pwf_wire::task::{CompleteTask, TaskRecord};

use crate::support::{FixedClock, InMemoryStore, project, task_record, task_timestamp};

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
        TaskVault::upsert_index_entry(&store, &foo(), entry).unwrap();
    }
    store
}

fn done_command(id: &str) -> CompleteTask {
    CompleteTask {
        id: id.parse().unwrap(),
        report: None,
        commits: None,
        expected_revision: None,
        request_id: None,
        request_fingerprint: None,
    }
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn done_evicts_the_oldest_note_completion_when_index_dates_are_absent(
    pool: sqlx::SqlitePool,
) {
    crate::support::insert_project(
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
            tasks.push(TaskRecord {
                completed_at: Some(task_timestamp(format!("2026-01-{:02}T00:00:00Z", 7 - n))),
                ..record(&format!("FOO-{n:04}"), TaskStatus::Done)
            });
            entry(&format!("FOO-{n:04}"), IndexEntryState::Done(None), "")
        })
        .collect();
    entries.push(entry("FOO-0007", IndexEntryState::Open, ""));
    let store = staged(tasks, entries);

    complete_task::execute(&done_command("FOO-0007"), &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Done);
    let marked = store
        .entries("foo-bar")
        .into_iter()
        .find(|e| e.id == TaskId::try_new("FOO-0007").unwrap())
        .unwrap();
    assert_eq!(
        marked.state,
        IndexEntryState::Done(Some(task_timestamp("2026-07-26T12:34:56Z")))
    );
    assert!(
        !store
            .entries("foo-bar")
            .iter()
            .any(|e| e.id == TaskId::try_new("FOO-0006").unwrap()),
        "evicted entry must be unlinked"
    );
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
    let store = staged(
        vec![record("FOO-0001", TaskStatus::Active)],
        vec![entry("FOO-0001", IndexEntryState::Open, "")],
    );
    complete_task::execute(&done_command("FOO-0001"), &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(
        store.tasks("foo-bar")[0].completed_at,
        Some(task_timestamp("2026-07-26T12:34:56Z"))
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn done_updates_an_unindexed_active_task(pool: sqlx::SqlitePool) {
    crate::support::insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let store = staged(vec![record("FOO-0001", TaskStatus::Active)], Vec::new());

    complete_task::execute(&done_command("FOO-0001"), &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Done);
    assert!(store.entries("foo-bar").is_empty());
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
    let store = staged(Vec::new(), Vec::new());

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
    let store = staged(
        vec![invalid],
        vec![entry("FOO-0001", IndexEntryState::Open, "")],
    );

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
    let store = staged(Vec::new(), Vec::new());

    let error = complete_task::execute(&done_command("XYZ-0001"), &store, &pool, &FixedClock)
        .await
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "Unknown project ID `XYZ` for task XYZ-0001"
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn completed_request_replays_its_minimal_result(pool: sqlx::SqlitePool) {
    use pwf_wire::task::{TaskRequestFingerprint, TaskRequestId};
    let request_id = TaskRequestId::try_new("request-1").unwrap();
    let fingerprint = TaskRequestFingerprint::from_digest([1; 32]);
    sqlx::query("INSERT INTO task_mutation_requests (request_id, operation, fingerprint, task_id, state, outcome, completed_at) VALUES (?, 'complete', ?, 'FOO-0001', 'completed', 'completed', '2026-07-26T12:34:56Z')")
        .bind(request_id.as_ref()).bind(fingerprint.as_ref()).execute(&pool).await.unwrap();
    let result = complete_task::execute(
        &CompleteTask {
            id: "FOO-0001".parse().unwrap(),
            report: None,
            commits: None,
            expected_revision: None,
            request_id: Some(request_id),
            request_fingerprint: Some(fingerprint),
        },
        &InMemoryStore::default(),
        &pool,
        &FixedClock,
    )
    .await
    .unwrap();
    assert!(result.task.is_none(), "legacy receipts have no summary");
}
