use pwf_application::{
    ports::user_settings::{
        TaskBodyPresetReader, UserSettingsConfigurationError, UserSettingsLoadError,
    },
    task::{
        TaskBodyTitleError,
        add_task::{self, AddTaskError},
        body_presets::{MarkerSectionItemStyle, TaskBodyPresets},
    },
};
use pwf_models::task::TaskStatus;
use pwf_wire::task::{AddTask, AddTaskBody, TaskMutationSummary};

use crate::support::{
    FixedClock, FixedTaskBodyPresets, InMemoryStore, InMemoryStoreFailure, blocked_by,
    insert_project, stored_blocked_by, task_record, task_timestamp,
};

struct InvalidTaskBodyPresets;

impl TaskBodyPresetReader for InvalidTaskBodyPresets {
    fn load_task_body_presets(&self) -> Result<TaskBodyPresets, UserSettingsLoadError> {
        Err(UserSettingsConfigurationError::new(
            "config.toml".into(),
            anyhow::anyhow!("unknown task body preset"),
        )
        .into())
    }
}

async fn registered_store(pool: &sqlx::SqlitePool) -> InMemoryStore {
    insert_project(pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    InMemoryStore::default().with_project_id("foo", "FOO")
}

fn command() -> AddTask {
    let source_id = "FOO-0001".parse::<pwf_models::task::TaskId>().unwrap();
    AddTask::new(
        source_id.project_id(),
        AddTaskBody::from_shorthand("ship it / do the thing"),
    )
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn add_inserts_task_file_and_returns_its_summary(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;

    let added = add_task::execute(
        command(),
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &FixedTaskBodyPresets::default(),
    )
    .await
    .unwrap();

    assert_eq!(added.outcome.as_ref(), "FOO-0001");
    assert_eq!(
        added.task,
        Some(TaskMutationSummary {
            id: added.outcome.clone(),
            title: "ship it".into(),
            status: TaskStatus::Active,
        })
    );
    assert_eq!(store.tasks("foo")[0].id, added.outcome);
    assert_eq!(store.tasks("foo").len(), 1);
    assert_eq!(
        store.tasks("foo")[0].created_at,
        Some(task_timestamp("2026-07-26T09:34:56-03:00"))
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn shorthand_add_accepts_only_a_title_and_normalizes_it_once(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let mut command = command();
    command.body = AddTaskBody::from_shorthand("  Web: Fix # metadata; keep case  ");

    let added = add_task::execute(
        command,
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &FixedTaskBodyPresets::default(),
    )
    .await
    .unwrap();

    assert_eq!(added.outcome.as_ref(), "FOO-0001");
    assert_eq!(
        store.tasks("foo")[0].title,
        "Web: Fix # metadata; keep case"
    );
    assert_eq!(store.tasks("foo")[0].body, "## Goals\n");
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn shorthand_add_uses_the_configured_preset_layout(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let mut command = command();
    command.body = AddTaskBody::from_shorthand(
        "custom title /o first goal / second goal /b custom context /g fallback context",
    );

    add_task::execute(
        command,
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &FixedTaskBodyPresets::selecting(&[
            ("/o", "Objectives", 3, MarkerSectionItemStyle::Numbered),
            ("/b", "Background", 4, MarkerSectionItemStyle::Paragraph),
        ]),
    )
    .await
    .unwrap();

    let task = &store.tasks("foo")[0];
    assert_eq!(task.title, "custom title");
    assert_eq!(
        task.body,
        "### Objectives\n\n1. first goal\n2. second goal\n\n#### Background\n\ncustom context\n\nfallback context"
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn add_reports_a_blocked_by_id_from_an_unknown_project(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let mut command = command();
    command.blocked_by = Some(blocked_by(&["MISS-0001"]));

    let error = add_task::execute(
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
        AddTaskError::UnknownBlockedByIds { ref ids }
            if ids.iter().map(AsRef::as_ref).collect::<Vec<_>>() == ["MISS-0001"]
    ));
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn add_accepts_a_blocker_from_a_paused_project(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    insert_project(
        &pool,
        "PAU",
        "paused-project",
        "/projects/paused",
        "/tasks/paused",
        true,
    )
    .await;
    let store = InMemoryStore::default()
        .with_project_id("foo", "FOO")
        .with_project("paused-project", vec![task_record("PAU-0001")]);
    let mut command = command();
    command.blocked_by = Some(blocked_by(&["PAU-0001"]));

    let added = add_task::execute(
        command,
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &FixedTaskBodyPresets::default(),
    )
    .await
    .unwrap();

    assert_eq!(added.outcome.as_ref(), "FOO-0001");
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn add_rejects_a_cycle_through_its_prospective_id_without_writing(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let origin = pwf_wire::task::TaskRecord {
        blocked_by: stored_blocked_by(&["FOO-0002"]),
        ..task_record("FOO-0001")
    };
    let store = InMemoryStore::default()
        .with_project_id("foo", "FOO")
        .with_project("foo", vec![origin]);
    let mut command = command();
    command.blocked_by = Some(blocked_by(&["FOO-0001"]));

    let error = add_task::execute(
        command,
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &FixedTaskBodyPresets::default(),
    )
    .await
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        "blocked_by cycle: FOO-0002 -> FOO-0001 -> FOO-0002"
    );
    assert!(matches!(
        &error,
        AddTaskError::BlockedByCycle { path }
            if path.iter().map(AsRef::as_ref).collect::<Vec<_>>()
                == ["FOO-0002", "FOO-0001", "FOO-0002"]
    ));
    assert_eq!(store.tasks("foo").len(), 1);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn add_uses_project_id_even_when_another_project_has_that_title(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "FOO",
        "original",
        "/projects/original",
        "/tasks/original",
        false,
    )
    .await;
    insert_project(&pool, "ALT", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store = InMemoryStore::default()
        .with_project_id("original", "FOO")
        .with_project_id("foo", "ALT");

    let added = add_task::execute(
        command(),
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &FixedTaskBodyPresets::default(),
    )
    .await
    .unwrap();

    assert_eq!(added.outcome.as_ref(), "FOO-0001");
    assert_eq!(store.tasks("original").len(), 1);
    assert!(store.tasks("foo").is_empty());
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn snapshots_only_reached_projects_once_including_paused_projects(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    insert_project(&pool, "AUX", "aux", "/work/aux", "/tasks/aux", true).await;
    insert_project(&pool, "BAD", "bad", "/work/bad", "/tasks/bad", false).await;
    sqlx::query("UPDATE projects SET tasks_path = '' WHERE id = 'BAD'")
        .execute(&pool)
        .await
        .unwrap();
    let mut first = task_record("FOO-0001");
    first.blocked_by = stored_blocked_by(&["AUX-0001"]);
    let mut second = task_record("FOO-0002");
    second.blocked_by = stored_blocked_by(&["AUX-0001"]);
    let mut shared = task_record("AUX-0001");
    shared.blocked_by = stored_blocked_by(&["FOO-0001", "FOO-0100"]);
    let store = InMemoryStore::default()
        .with_project_id("foo", "FOO")
        .with_project("foo", vec![first, second, task_record("FOO-0099")])
        .with_project("aux", vec![shared])
        .with_failure(InMemoryStoreFailure::ReadTaskRecord)
        .with_failure(InMemoryStoreFailure::ListTasks);
    let mut command = command();
    command.blocked_by = Some(blocked_by(&["FOO-0001", "FOO-0002"]));
    let error = add_task::execute(
        command,
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &FixedTaskBodyPresets::default(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, AddTaskError::BlockedByCycle { .. }));
    let mut reads = store.dependency_reads();
    reads.sort();
    assert_eq!(
        reads.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
        ["AUX", "FOO"]
    );
    assert_eq!(store.tasks("foo").len(), 3);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn missing_projects_and_task_files_do_not_supply_dependencies(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let mut command = command();
    command.blocked_by = Some(blocked_by(&["FOO-0002", "AUX-0001"]));
    let error = add_task::execute(
        command,
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &FixedTaskBodyPresets::default(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Unknown --blocked-by id(s): FOO-0002, AUX-0001."
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn dependency_read_failures_keep_the_task_id_and_source(pool: sqlx::SqlitePool) {
    use std::error::Error as _;
    let store = registered_store(&pool)
        .await
        .with_failure(InMemoryStoreFailure::ReadTask);
    let mut command = command();
    command.blocked_by = Some(blocked_by(&["AUX-0001"]));
    insert_project(&pool, "AUX", "aux", "/work/aux", "/tasks/aux", false).await;
    let error = add_task::execute(
        command,
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &FixedTaskBodyPresets::default(),
    )
    .await
    .unwrap_err();
    assert!(matches!(&error, AddTaskError::ReadBlockedBy { id, .. } if id.as_ref() == "AUX-0001"));
    assert_eq!(
        error.source().unwrap().to_string(),
        "injected in-memory store failure: task-read"
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn seeded_configuration_preserves_the_current_markers_and_headers(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let mut command = command();
    command.body = AddTaskBody::from_shorthand("title / goal /c context /n constraint /d done");
    add_task::execute(
        command,
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &FixedTaskBodyPresets::default(),
    )
    .await
    .unwrap();
    let task = &store.tasks("foo")[0];
    assert_eq!(task.title, "title");
    assert_eq!(
        task.body,
        "## Goals\n\n- goal\n\n## Context\n\n- context\n\n## Constraints\n\n- constraint\n\n## Done When\n\n- done"
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn shorthand_add_renders_explicit_empty_sections_once(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let mut empty_context = command();
    empty_context.body = AddTaskBody::from_shorthand("empty context /c");

    add_task::execute(
        empty_context,
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &FixedTaskBodyPresets::default(),
    )
    .await
    .unwrap();

    let mut repeated_context = command();
    repeated_context.body = AddTaskBody::from_shorthand("my task /c some context /d /c");

    add_task::execute(
        repeated_context,
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &FixedTaskBodyPresets::default(),
    )
    .await
    .unwrap();

    let tasks = store.tasks("foo");
    assert_eq!(tasks[0].body, "## Goals\n\n\n## Context\n");
    assert_eq!(
        tasks[1].body,
        "## Goals\n\n\n## Context\n\n- some context\n\n## Done When\n"
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn invalid_task_body_settings_reject_creation(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let error = add_task::execute(
        command(),
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &InvalidTaskBodyPresets,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        AddTaskError::TaskBodyPresets(UserSettingsLoadError::InvalidConfiguration(_))
    ));
    assert!(store.tasks("foo").is_empty());
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn shorthand_still_requires_a_title(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    for body in ["  ", "/g only a goal"] {
        let mut command = command();
        command.body = AddTaskBody::from_shorthand(body);
        let error = add_task::execute(
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
            AddTaskError::InvalidTitle(TaskBodyTitleError::Missing)
        ));
        assert!(store.tasks("foo").is_empty());
    }
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn repeated_creation_allocates_distinct_task_files(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let first = add_task::execute(
        command(),
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &FixedTaskBodyPresets::default(),
    )
    .await
    .unwrap();
    let second = add_task::execute(
        command(),
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &FixedTaskBodyPresets::default(),
    )
    .await
    .unwrap();

    assert_eq!(first.outcome.as_ref(), "FOO-0001");
    assert_eq!(second.outcome.as_ref(), "FOO-0002");
    assert_eq!(store.tasks("foo").len(), 2);
}
