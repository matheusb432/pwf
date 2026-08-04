use pwf_models::{
    project::ProjectId,
    task::{ProjectName, TaskId, TaskStatus},
};

use super::{
    resolve_task_project::{self, ResolveTaskProject, ResolveTaskProjectError},
    store_util::{self, LoadTaskError},
};
use crate::ports::task_record::{
    IndexEntry, IndexEntryState, IndexEntryStore, TaskPatch, TaskStore,
};

#[derive(Debug, Clone)]
pub struct ReopenTask {
    pub id: TaskId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReopenTaskOk {
    pub id: TaskId,
    pub project: ProjectName,
    pub already_active: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ReopenTaskError {
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error("Unknown project ID `{project_id}` for task {task_id}")]
    UnknownProjectId {
        task_id: TaskId,
        project_id: ProjectId,
    },
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    QueryProject(Box<dyn std::error::Error + Send + Sync>),
}

/// Reopens a closed task and restores an existing queue link.
///
/// The update clears `completed:` and `commits:`. An active task returns an idempotent skip.
#[cqrsy::command]
pub async fn execute(
    cmd: &ReopenTask,
    store: &(impl TaskStore + IndexEntryStore),
    pool: &sqlx::SqlitePool,
) -> Result<ReopenTaskOk, ReopenTaskError> {
    let resolved = resolve_task_project::execute(ResolveTaskProject { id: cmd.id.clone() }, pool)
        .await
        .map_err(|error| match error {
            ResolveTaskProjectError::UnknownProjectId {
                task_id,
                project_id,
            } => ReopenTaskError::UnknownProjectId {
                task_id,
                project_id,
            },
            ResolveTaskProjectError::QueryProject(source) => ReopenTaskError::QueryProject(source),
        })?;
    let task_identifier = resolved.id;
    let project = resolved.project;
    let record =
        store_util::require_task(store, &project, &task_identifier).map_err(
            |error| match error {
                LoadTaskError::TaskNotFound { id } => ReopenTaskError::TaskNotFound { id },
                LoadTaskError::Store(source) => ReopenTaskError::WriteStore(source),
            },
        )?;

    if record.status == TaskStatus::Active {
        return Ok(ReopenTaskOk {
            id: task_identifier,
            project: project.title.clone(),
            already_active: true,
        });
    }

    TaskStore::update(
        store,
        &project,
        &task_identifier,
        TaskPatch {
            status: Some(TaskStatus::Active),
            completed: Some(None),
            commits: Some(None),
            ..TaskPatch::default()
        },
    )
    .map_err(|error| ReopenTaskError::WriteStore(Box::new(error)))?;

    let entries = IndexEntryStore::list_index_entries(store, &project)
        .map_err(|error| ReopenTaskError::WriteStore(Box::new(error)))?;
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
                section: String::new(),
            },
        )
        .map_err(|error| ReopenTaskError::WriteStore(Box::new(error)))?;
    }

    Ok(ReopenTaskOk {
        id: task_identifier,
        project: project.title.clone(),
        already_active: false,
    })
}

#[cfg(test)]
mod tests {
    use pwf_models::{
        project::Project,
        task::{TaskId, TaskStatus, Timestamp},
    };

    use super::ReopenTask;
    use crate::{
        ports::task_record::{
            IndexEntry, IndexEntryState, IndexEntryStore, Materialization, TaskRecord,
        },
        testing::{InMemoryStore, project},
    };

    fn record(id: &str, status: TaskStatus) -> TaskRecord {
        TaskRecord {
            id: TaskId::try_new(id).unwrap(),
            title: "tray gui".to_string(),
            status,
            created: Some(Timestamp::new("2026-01-01")),
            completed: (status != TaskStatus::Active).then(|| Timestamp::new("2026-01-02")),
            commits: Some("a..b".to_string()),
            tags: None,
            effort: None,
            prereq: None,
            section: None,
            body: "\nbody\n".to_string(),
            source: "body".to_string(),
            locator: format!("/mem/foo-bar/{id}.md"),
            placement: None,
            materialization: Materialization::NoteFile,
        }
    }

    fn foo() -> Project {
        project("FOO".parse().unwrap(), "foo-bar")
    }

    fn staged(status: TaskStatus, entries: Vec<IndexEntry>) -> InMemoryStore {
        let store = InMemoryStore::default()
            .with_project_id("foo-bar", "FOO".parse().unwrap())
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
            section: String::new(),
        }
    }

    fn command() -> ReopenTask {
        ReopenTask {
            id: "FOO-0001".parse().unwrap(),
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn reopen_restores_done_entry(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO".parse().unwrap(),
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(
            TaskStatus::Done,
            vec![entry(IndexEntryState::Done(Timestamp::new("2026-01-02")))],
        );

        let out = super::execute(&command(), &store, &pool).await.unwrap();

        assert!(!out.already_active);
        assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Active);
        assert_eq!(store.tasks("foo-bar")[0].completed, None);
        assert_eq!(store.tasks("foo-bar")[0].commits, None);
        assert_eq!(store.entries("foo-bar")[0].state, IndexEntryState::Open);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn reopen_preserves_absent_index_entry(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO".parse().unwrap(),
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(TaskStatus::Done, Vec::new());

        let out = super::execute(&command(), &store, &pool).await.unwrap();

        assert!(!out.already_active);
        assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Active);
        assert!(store.entries("foo-bar").is_empty());
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn reopen_already_active_is_idempotent_skip(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO".parse().unwrap(),
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(TaskStatus::Active, vec![entry(IndexEntryState::Open)]);

        let out = super::execute(&command(), &store, &pool).await.unwrap();

        assert!(out.already_active);
        assert_eq!(store.tasks("foo-bar")[0].commits.as_deref(), Some("a..b"));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn reopen_reports_an_unknown_project_id(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO".parse().unwrap(),
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(TaskStatus::Done, Vec::new());
        let command = ReopenTask {
            id: "XYZ-0001".parse().unwrap(),
        };

        let error = super::execute(&command, &store, &pool).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown project ID `XYZ` for task XYZ-0001"
        );
    }
}
