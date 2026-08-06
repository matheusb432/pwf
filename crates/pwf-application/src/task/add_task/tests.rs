use pwf_models::task::{TaskTitle, Timestamp};

use super::{AddTask, AddTaskError, AddTaskPrompt};
use crate::{
    ports::{clock::Clock, task_record::IndexEntryState},
    task::TaskLanes,
    testing::{FixedClock, InMemoryStore, insert_project},
};

async fn execute(
    command: &AddTask,
    store: &InMemoryStore,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<super::AddTaskOk, AddTaskError> {
    super::execute(command, store, pool, clock).await
}

fn task_title(raw: &str) -> TaskTitle {
    TaskTitle::try_new(raw).unwrap()
}

fn command() -> AddTask {
    AddTask {
        project_selector: Some("pwf".parse().unwrap()),
        prompt: AddTaskPrompt::Structured {
            title: task_title("ship it"),
            lanes: TaskLanes::try_new(
                vec!["do the thing".to_string()],
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .unwrap(),
        },
        human: false,
        prerequisites: None,
        effort: None,
        tags: None,
    }
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_inserts_record_and_open_index_entry(pool: sqlx::SqlitePool) {
    insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
    let store = InMemoryStore::default().with_project_id("pwf", "PWF");

    let added = execute(&command(), &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(added.id.as_ref(), "PWF-0001");
    assert_eq!(added.project, "pwf");
    assert_eq!(added.title, "ship it");
    assert_eq!(added.created_section, None);
    let entries = store.entries("pwf");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id.as_ref(), "PWF-0001");
    assert_eq!(entries[0].state, IndexEntryState::Open);
    assert_eq!(store.tasks("pwf").len(), 1);
    assert_eq!(
        store.tasks("pwf")[0].created,
        Some(Timestamp::new("2026-07-26"))
    );
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_forwards_an_explicit_task_title(pool: sqlx::SqlitePool) {
    insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
    let store = InMemoryStore::default().with_project_id("pwf", "PWF");
    let mut command = command();
    command.prompt = AddTaskPrompt::Structured {
        title: task_title("fix # metadata"),
        lanes: TaskLanes::default(),
    };

    let added = execute(&command, &store, &pool, &FixedClock).await.unwrap();

    assert_eq!(added.title, "fix  metadata");
    assert_eq!(store.tasks("pwf")[0].title, "fix  metadata");
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_inferred_prompt_title_is_normalized_once(pool: sqlx::SqlitePool) {
    insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
    let store = InMemoryStore::default().with_project_id("pwf", "PWF");
    let mut command = command();
    command.prompt = AddTaskPrompt::Shorthand("fix # metadata".to_string());

    let added = execute(&command, &store, &pool, &FixedClock).await.unwrap();

    assert_eq!(added.title, "fix  metadata");
    assert_eq!(store.tasks("pwf")[0].title, "fix  metadata");
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_uses_the_clock_date(pool: sqlx::SqlitePool) {
    insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
    let store = InMemoryStore::default().with_project_id("pwf", "PWF");
    execute(&command(), &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(
        store.tasks("pwf")[0].created,
        Some(Timestamp::new("2026-07-26"))
    );
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_reports_created_section_only_when_region_absent(pool: sqlx::SqlitePool) {
    insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
    let store = InMemoryStore::default().with_project_id("pwf", "PWF");

    let mut command = command();
    command.human = true;
    let added = execute(&command, &store, &pool, &FixedClock).await.unwrap();

    assert_eq!(added.created_section.as_deref(), Some("Human"));
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn structured_add_renders_lane_values_without_shorthand_parsing(pool: sqlx::SqlitePool) {
    insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
    let store = InMemoryStore::default().with_project_id("pwf", "PWF");
    let mut command = command();
    command.prompt = AddTaskPrompt::Structured {
        title: task_title("machine prompt"),
        lanes: TaskLanes::try_new(
            vec!["keep /d literal".to_string()],
            vec!["known context".to_string()],
            Vec::new(),
            Vec::new(),
        )
        .unwrap(),
    };

    let added = execute(&command, &store, &pool, &FixedClock).await.unwrap();

    assert_eq!(added.title, "machine prompt");
    assert_eq!(
        store.tasks("pwf")[0].body,
        "## Goals\n\n- keep /d literal\n\n## Context\n\n- known context"
    );
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_does_not_report_created_section_for_existing_empty_region(pool: sqlx::SqlitePool) {
    insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
    let store = InMemoryStore::default()
        .with_project_id("pwf", "PWF")
        .with_sections("pwf", &["Human"]);

    let mut command = command();
    command.human = true;
    let added = execute(&command, &store, &pool, &FixedClock).await.unwrap();

    assert_eq!(added.created_section, None);
}
