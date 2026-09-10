use pwf_application::task::{get_task_record, get_task_record::GetTaskRecordError};
use pwf_wire::task::{StoredBlockedBy, TaskRecord};

use crate::support::{FOO_0001_SOURCE, InMemoryStore, insert_project, staged_task, task_record};

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn retrieval_preserves_malformed_metadata(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let record = TaskRecord {
        title: "x".repeat(201),
        effort: Some("extreme".to_string()),
        priority: Some("urgent".to_string()),
        blocked_by: StoredBlockedBy::Malformed {
            raw: "bad links".to_string(),
            reason: "expected a sequence".to_string(),
        },
        ..task_record("FOO-0001")
    };
    let store = InMemoryStore::default().with_project("foo", vec![record.clone()]);
    let result = get_task_record::execute(&record.id, &store, &pool)
        .await
        .unwrap();
    assert_eq!(result, record);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn retrieval_preserves_source_and_revision(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let (store, _) = staged_task();
    let record = get_task_record::execute(&"FOO-0001".parse().unwrap(), &store, &pool)
        .await
        .unwrap();
    assert_eq!(record.source, FOO_0001_SOURCE);
    assert_eq!(record.revision.as_ref().len(), 64);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn absent_tasks_and_ineligible_projects_are_not_found(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    insert_project(&pool, "BAR", "bar", "/projects/bar", "/tasks/bar", true).await;
    let store = InMemoryStore::default().with_project("bar", vec![task_record("BAR-0001")]);
    for id in ["FOO-0001", "BAR-0001", "MISS-0001"] {
        let error = get_task_record::execute(&id.parse().unwrap(), &store, &pool)
            .await
            .unwrap_err();
        assert!(matches!(error, GetTaskRecordError::TaskNotFound { .. }));
    }
}
