use pwf_application::task::{
    TaskPromptLanesError, TaskPromptTitleError,
    add_task::{self, AddTaskError},
};
use pwf_models::task::{TaskStatus, TaskTitle};
use pwf_wire::task::{AddTask, AddTaskPrompt, TaskLanes, TaskMutationSummary};

use crate::support::{
    FixedClock, InMemoryStore, InMemoryStoreFailure, blocked_by, insert_project, stored_blocked_by,
    task_record, task_timestamp,
};

fn task_title(raw: &str) -> TaskTitle {
    TaskTitle::try_new(raw).unwrap()
}

async fn registered_store(pool: &sqlx::SqlitePool) -> InMemoryStore {
    insert_project(pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    InMemoryStore::default().with_project_id("foo", "FOO")
}

fn command() -> AddTask {
    let source_id = "FOO-0001".parse::<pwf_models::task::TaskId>().unwrap();
    AddTask::new(
        source_id.project_id(),
        AddTaskPrompt::from_structured(
            task_title("ship it"),
            TaskLanes::try_new(
                vec!["do the thing".to_string()],
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .unwrap(),
        ),
    )
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn add_inserts_task_note_and_returns_its_summary(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;

    let added = add_task::execute(command(), &store, &pool, &FixedClock)
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
        Some(task_timestamp("2026-07-26T12:34:56Z"))
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn add_forwards_an_explicit_task_title(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let mut command = command();
    command.prompt =
        AddTaskPrompt::from_structured(task_title("fix # metadata"), TaskLanes::default());

    let added = add_task::execute(command, &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(added.outcome.as_ref(), "FOO-0001");
    assert_eq!(store.tasks("foo")[0].title, "fix  metadata");
    assert_eq!(store.tasks("foo")[0].body, "## Goals\n");
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn shorthand_add_accepts_only_a_title_and_normalizes_it_once(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let mut command = command();
    command.prompt = AddTaskPrompt::from_shorthand("  fix # metadata  ");

    let added = add_task::execute(command, &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(added.outcome.as_ref(), "FOO-0001");
    assert_eq!(store.tasks("foo")[0].title, "fix  metadata");
    assert_eq!(store.tasks("foo")[0].body, "## Goals\n");
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn shorthand_add_uses_runtime_markers_and_headers(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    sqlx::query(
        "UPDATE task_prompt_lanes SET marker = '/o', header = 'Objectives' WHERE lane = 'goals'",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE task_prompt_lanes SET marker = '/b', header = 'Background' WHERE lane = 'context'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let mut command = command();
    command.prompt = AddTaskPrompt::from_shorthand(
        "custom title /o custom goal /b custom context /g fallback context",
    );

    add_task::execute(command, &store, &pool, &FixedClock)
        .await
        .unwrap();

    let task = &store.tasks("foo")[0];
    assert_eq!(task.title, "custom title");
    assert_eq!(
        task.body,
        "## Objectives\n\n- custom goal\n\n## Background\n\n- custom context\n- fallback context"
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn structured_add_renders_lane_values_without_shorthand_parsing(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let mut command = command();
    command.prompt = AddTaskPrompt::from_structured(
        task_title("machine prompt"),
        TaskLanes::try_new(
            vec!["keep /d literal".to_string()],
            vec!["known context".to_string()],
            Vec::new(),
            Vec::new(),
        )
        .unwrap(),
    );

    let added = add_task::execute(command, &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(added.outcome.as_ref(), "FOO-0001");
    assert_eq!(
        store.tasks("foo")[0].body,
        "## Goals\n\n- keep /d literal\n\n## Context\n\n- known context"
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn add_reports_a_blocked_by_id_from_an_unknown_project(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let mut command = command();
    command.blocked_by = Some(blocked_by(&["MISS-0001"]));

    let error = add_task::execute(command, &store, &pool, &FixedClock)
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

    let added = add_task::execute(command, &store, &pool, &FixedClock)
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

    let error = add_task::execute(command, &store, &pool, &FixedClock)
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

    let added = add_task::execute(command(), &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(added.outcome.as_ref(), "FOO-0001");
    assert_eq!(store.tasks("original").len(), 1);
    assert!(store.tasks("foo").is_empty());
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn reads_only_reachable_dependencies_once_including_paused_projects(pool: sqlx::SqlitePool) {
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
    let error = add_task::execute(command, &store, &pool, &FixedClock)
        .await
        .unwrap_err();
    assert!(matches!(error, AddTaskError::BlockedByCycle { .. }));
    let mut reads = store.dependency_reads();
    reads.sort();
    assert_eq!(
        reads.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
        ["AUX-0001", "FOO-0001", "FOO-0002"]
    );
    assert_eq!(store.tasks("foo").len(), 3);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn missing_projects_and_task_notes_do_not_supply_dependencies(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let mut command = command();
    command.blocked_by = Some(blocked_by(&["FOO-0002", "AUX-0001"]));
    let error = add_task::execute(command, &store, &pool, &FixedClock)
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
    let error = add_task::execute(command, &store, &pool, &FixedClock)
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
    command.prompt = AddTaskPrompt::from_shorthand("title / goal /c context /n constraint /d done");
    add_task::execute(command, &store, &pool, &FixedClock)
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
async fn missing_lane_rejects_creation(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    sqlx::query("DELETE FROM task_prompt_lanes WHERE lane = 'constraints'")
        .execute(&pool)
        .await
        .unwrap();
    let error = add_task::execute(command(), &store, &pool, &FixedClock)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        AddTaskError::PromptLanes(TaskPromptLanesError::InvalidLaneSet { .. })
    ));
    assert!(store.tasks("foo").is_empty());
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn shorthand_still_requires_a_title(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    for prompt in ["  ", "/g only a goal"] {
        let mut command = command();
        command.prompt = AddTaskPrompt::from_shorthand(prompt);
        let error = add_task::execute(command, &store, &pool, &FixedClock)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            AddTaskError::InvalidTitle(TaskPromptTitleError::Missing)
        ));
        assert!(store.tasks("foo").is_empty());
    }
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn repeated_creation_allocates_distinct_task_notes(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let first = add_task::execute(command(), &store, &pool, &FixedClock)
        .await
        .unwrap();
    let second = add_task::execute(command(), &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(first.outcome.as_ref(), "FOO-0001");
    assert_eq!(second.outcome.as_ref(), "FOO-0002");
    assert_eq!(store.tasks("foo").len(), 2);
}
