use pwf_application::task::{backlog_task, get_task_record};
use pwf_models::task::{TaskId, TaskStatus};
use pwf_wire::task::{BacklogTaskOutcome, TaskRecord};

use crate::support::{InMemoryStore, InMemoryStoreFailure, insert_project, task_record};

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn backlog_preserves_task_content_and_repeating_it_does_not_write(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let id = TaskId::try_new("FOO-0001").unwrap();
    let original = TaskRecord {
        effort: Some("unrelated invalid metadata".to_string()),
        body: "## Goals\n\n- defer this work\n".to_string(),
        ..task_record(id.as_ref())
    };
    let store = InMemoryStore::default()
        .with_project("foo-bar", vec![original.clone()])
        .with_failure(InMemoryStoreFailure::ListTasks);

    let result = backlog_task::execute(&id, &store, &pool).await.unwrap();

    assert_eq!(result.outcome, BacklogTaskOutcome::Backlogged);
    assert_eq!(result.task.unwrap().status, TaskStatus::Backlog);
    let record = get_task_record::execute(&id, &store, &pool).await.unwrap();
    assert_eq!(record.status, TaskStatus::Backlog);
    assert_eq!(record.body, original.body);
    assert_eq!(record.effort, original.effort);
    assert_eq!(record.created_at, original.created_at);
    assert_eq!(record.completed_at, original.completed_at);

    let result = backlog_task::execute(&id, &store, &pool).await.unwrap();
    assert_eq!(result.outcome, BacklogTaskOutcome::AlreadyBacklogged);
    assert_eq!(
        get_task_record::execute(&id, &store, &pool).await.unwrap(),
        record
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn backlog_rejects_closed_tasks_without_changing_them(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "FOO",
        "foo-bar",
        "/projects/foo",
        "/tasks/foo",
        false,
    )
    .await;
    let id = TaskId::try_new("FOO-0001").unwrap();
    for status in [TaskStatus::Done, TaskStatus::Cancelled] {
        let record = TaskRecord {
            status,
            ..task_record(id.as_ref())
        };
        let store = InMemoryStore::default().with_project("foo-bar", vec![record.clone()]);
        let error = backlog_task::execute(&id, &store, &pool).await.unwrap_err();
        assert!(matches!(
            error,
            backlog_task::BacklogTaskError::ClosedTask { .. }
        ));
        assert_eq!(
            get_task_record::execute(&id, &store, &pool).await.unwrap(),
            record
        );
    }
}
