use pwf_models::task::{IndexSection, TaskPrompt, TaskTimestamp, TaskTitle};
use pwf_wire::task::{AddTask, AddTaskPrompt, TaskLanes};

use crate::{
    ports::task_record::IndexEntryState,
    task::add_task::{self, AddTaskError},
    testing::{
        FixedClock, InMemoryStore, blocked_by, insert_project, stored_blocked_by, task_record,
    },
};

fn task_title(raw: &str) -> TaskTitle {
    TaskTitle::try_new(raw).unwrap()
}

fn task_timestamp(raw: &str) -> TaskTimestamp {
    raw.parse().unwrap()
}

async fn registered_store(pool: &sqlx::SqlitePool) -> InMemoryStore {
    insert_project(pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    InMemoryStore::default().with_project_id("foo", "FOO")
}

fn command() -> AddTask {
    AddTask {
        project_selector: "foo".parse().unwrap(),
        prompt: AddTaskPrompt::structured(
            task_title("ship it"),
            TaskLanes::try_new(
                vec!["do the thing".to_string()],
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .unwrap(),
        ),
        index_section: IndexSection::default(),
        blocked_by: None,
        effort: None,
        tags: None,
    }
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_inserts_record_and_open_index_entry(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;

    let added = add_task::execute(&command(), &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(added.id.as_ref(), "FOO-0001");
    assert_eq!(added.project.as_ref(), "foo");
    assert_eq!(added.title.as_ref(), "ship it");
    assert_eq!(added.created_section, None);
    let entries = store.entries("foo");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id.as_ref(), "FOO-0001");
    assert_eq!(entries[0].state, IndexEntryState::Open);
    assert_eq!(store.tasks("foo").len(), 1);
    assert_eq!(
        store.tasks("foo")[0].created_at,
        Some(task_timestamp("2026-07-26T12:34:56Z"))
    );
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_forwards_an_explicit_task_title(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let mut command = command();
    command.prompt = AddTaskPrompt::structured(task_title("fix # metadata"), TaskLanes::default());

    let added = add_task::execute(&command, &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(added.title.as_ref(), "fix  metadata");
    assert_eq!(store.tasks("foo")[0].title, "fix  metadata");
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_inferred_prompt_title_is_normalized_once(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let mut command = command();
    command.prompt = AddTaskPrompt::shorthand(TaskPrompt::new("fix # metadata")).unwrap();

    let added = add_task::execute(&command, &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(added.title.as_ref(), "fix  metadata");
    assert_eq!(store.tasks("foo")[0].title, "fix  metadata");
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_uses_the_clock_timestamp(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    add_task::execute(&command(), &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(
        store.tasks("foo")[0].created_at,
        Some(task_timestamp("2026-07-26T12:34:56Z"))
    );
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_reports_created_section_only_when_region_absent(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;

    let mut command = command();
    command.index_section = IndexSection::Human;
    let added = add_task::execute(&command, &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(
        added.created_section.as_ref().map(AsRef::as_ref),
        Some("Human")
    );
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn structured_add_renders_lane_values_without_shorthand_parsing(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let mut command = command();
    command.prompt = AddTaskPrompt::structured(
        task_title("machine prompt"),
        TaskLanes::try_new(
            vec!["keep /d literal".to_string()],
            vec!["known context".to_string()],
            Vec::new(),
            Vec::new(),
        )
        .unwrap(),
    );

    let added = add_task::execute(&command, &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(added.title.as_ref(), "machine prompt");
    assert_eq!(
        store.tasks("foo")[0].body,
        "## Goals\n\n- keep /d literal\n\n## Context\n\n- known context"
    );
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_does_not_report_created_section_for_existing_empty_region(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool)
        .await
        .with_sections("foo", &["Human"]);

    let mut command = command();
    command.index_section = IndexSection::Human;
    let added = add_task::execute(&command, &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(added.created_section, None);
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_reports_a_blocked_by_id_from_an_unknown_project(pool: sqlx::SqlitePool) {
    let store = registered_store(&pool).await;
    let mut command = command();
    command.blocked_by = Some(blocked_by(&["MISS-0001"]));

    let error = add_task::execute(&command, &store, &pool, &FixedClock)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        AddTaskError::UnknownBlockedByIds { ref ids }
            if ids.iter().map(AsRef::as_ref).collect::<Vec<_>>() == ["MISS-0001"]
    ));
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
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

    let added = add_task::execute(&command, &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(added.id.as_ref(), "FOO-0001");
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_rejects_a_cycle_through_its_prospective_id_without_writing(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let origin = crate::ports::task_record::TaskRecord {
        blocked_by: stored_blocked_by(&["FOO-0002"]),
        ..task_record("FOO-0001")
    };
    let store = InMemoryStore::default()
        .with_project_id("foo", "FOO")
        .with_project("foo", vec![origin]);
    let mut command = command();
    command.blocked_by = Some(blocked_by(&["FOO-0001"]));

    let error = add_task::execute(&command, &store, &pool, &FixedClock)
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
    assert!(store.entries("foo").is_empty());
}
