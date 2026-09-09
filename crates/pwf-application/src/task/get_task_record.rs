use pwf_models::task::TaskId;
use pwf_wire::task::TaskRecord;

use crate::{
    ports::task_vault::TaskVault,
    project::{get_active_project, get_project::GetProjectError},
};

#[derive(Debug, thiserror::Error)]
pub enum GetTaskRecordError {
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error(transparent)]
    ReadStore(anyhow::Error),
    #[error(transparent)]
    QueryProject(anyhow::Error),
}

#[cqrsy::query]
pub async fn execute(
    id: &TaskId,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
) -> Result<TaskRecord, GetTaskRecordError> {
    let project = match get_active_project::execute(id.project_id(), pool).await {
        Ok(project) => project,
        Err(GetProjectError::ProjectNotFound { .. }) => {
            return Err(GetTaskRecordError::TaskNotFound { id: id.clone() });
        }
        Err(error) => return Err(GetTaskRecordError::QueryProject(anyhow::Error::new(error))),
    };
    store
        .get_task_record(&project, id)
        .map_err(|error| GetTaskRecordError::ReadStore(anyhow::Error::new(error)))?
        .ok_or_else(|| GetTaskRecordError::TaskNotFound { id: id.clone() })
}

#[cfg(test)]
mod tests {
    use pwf_wire::task::{Materialization, StoredBlockedBy, TaskRecord};

    use super::GetTaskRecordError;
    use crate::{
        task::get_task_record,
        testing::{
            FOO_0001_SOURCE, InMemoryStore, InMemoryStoreFailure, insert_project,
            staged_missing_task, staged_task, task_record,
        },
    };

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
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

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn retrieval_preserves_source_and_revision(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let (store, _) = staged_task();
        let record = get_task_record::execute(&"FOO-0001".parse().unwrap(), &store, &pool)
            .await
            .unwrap();
        assert_eq!(record.source, FOO_0001_SOURCE);
        assert_eq!(record.revision.as_ref().len(), 64);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn retrieval_returns_an_index_only_record_without_a_second_read(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let (store, _) = staged_missing_task();
        let store = store.with_failure(InMemoryStoreFailure::ReadTaskMarkdown);
        let record = get_task_record::execute(&"FOO-0002".parse().unwrap(), &store, &pool)
            .await
            .unwrap();
        assert!(matches!(
            record.materialization,
            Materialization::MissingNote { .. }
        ));
        assert_eq!(
            record.locator.as_path(),
            std::path::Path::new("/notes/foo/FOO-0002.md")
        );
        assert!(record.source.is_empty());
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
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
}
