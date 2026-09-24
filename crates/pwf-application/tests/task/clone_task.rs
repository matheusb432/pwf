use pwf_application::{
    ports::user_settings::{TaskBodyPresetReader, UserSettingsLoadError},
    task::{
        body_presets::TaskBodyPresets,
        clone_task::{self, CloneTaskError},
        get_task::{self, GetTaskError},
    },
};
use pwf_wire::task::{CloneTask, ClonedTaskProjectId, TaskRecord, TaskRecordError};

use crate::support::{
    FixedClock, FixedTaskBodyPresets, InMemoryStore, insert_project, task_record,
};

/// Proves verbatim creation paths never read task-body presets.
struct UnreadableTaskBodyPresets;

impl TaskBodyPresetReader for UnreadableTaskBodyPresets {
    fn load_task_body_presets(&self) -> Result<TaskBodyPresets, UserSettingsLoadError> {
        Err(anyhow::anyhow!("task body presets must not be read").into())
    }
}

async fn staged(pool: &sqlx::SqlitePool, source: TaskRecord) -> (InMemoryStore, CloneTask) {
    insert_project(pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let command = CloneTask {
        id: source.id.clone(),
        project_id: ClonedTaskProjectId::SameAsTask,
    };
    let store = InMemoryStore::default()
        .with_project_id("foo", "FOO")
        .with_project("foo", vec![source]);
    (store, command)
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn clone_keeps_the_source_project_title_and_authored_body(pool: sqlx::SqlitePool) {
    let source = TaskRecord {
        body: "  authored /g text must stay literal\n".into(),
        ..task_record("FOO-0001")
    };
    let (store, command) = staged(&pool, source.clone()).await;

    let result = clone_task::execute(
        command,
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &UnreadableTaskBodyPresets,
    )
    .await
    .unwrap();
    let cloned = get_task::execute(
        &result.outcome,
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
    )
    .await
    .unwrap();

    assert_ne!(cloned.id, source.id);
    assert_eq!(cloned.id.project_id(), source.id.project_id());
    assert_eq!(cloned.title.as_ref(), source.title);
    assert_eq!(cloned.body.as_ref(), source.body);
    assert_eq!(store.tasks("foo").len(), 2);
    assert_eq!(store.tasks("foo")[0], source);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn clone_rejects_an_invalid_source_without_writing(pool: sqlx::SqlitePool) {
    let source = TaskRecord {
        title: "x".repeat(201),
        ..task_record("FOO-0001")
    };
    let (store, command) = staged(&pool, source.clone()).await;

    let error = clone_task::execute(
        command,
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &FixedTaskBodyPresets::default(),
    )
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        CloneTaskError::GetTask(GetTaskError::Parse(TaskRecordError::Metadata {
            field: "title",
            ..
        }))
    ));
    assert_eq!(store.tasks("foo"), [source]);
}
