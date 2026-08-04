use pwf_models::task::{TaskTitle, Timestamp};

use super::{AddTask, AddTaskError, plan_title};
use crate::{
    ports::{clock::Clock, task_record::IndexEntryState},
    testing::{InMemoryStore, insert_project},
};

#[derive(Clone)]
struct FixedClock;

impl Clock for FixedClock {
    fn today(&self) -> Timestamp {
        Timestamp::new("2026-07-26")
    }
}

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

fn command(section: Option<&str>) -> AddTask {
    AddTask {
        project_selector: Some("pwf".parse().unwrap()),
        prompt: "do the thing".to_string(),
        continue_path: None,
        title: Some(task_title("ship it")),
        date: Some("2026-07-15".parse().unwrap()),
        section: section.map(str::to_string),
        human: false,
        prerequisites: None,
        effort: None,
        tags: Vec::new(),
    }
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_inserts_record_and_open_index_entry(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "PWF".parse().unwrap(),
        "pwf",
        "/projects/pwf",
        "/tasks/pwf",
        false,
    )
    .await;
    let store = InMemoryStore::default().with_project_id("pwf", "PWF".parse().unwrap());

    let added = execute(&command(None), &store, &pool, &FixedClock)
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
        Some(Timestamp::new("2026-07-15"))
    );
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_forwards_an_explicit_task_title(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "PWF".parse().unwrap(),
        "pwf",
        "/projects/pwf",
        "/tasks/pwf",
        false,
    )
    .await;
    let store = InMemoryStore::default().with_project_id("pwf", "PWF".parse().unwrap());
    let mut command = command(None);
    command.title = Some(task_title("fix # metadata"));

    let added = execute(&command, &store, &pool, &FixedClock).await.unwrap();

    assert_eq!(added.title, "fix  metadata");
    assert_eq!(store.tasks("pwf")[0].title, "fix  metadata");
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_inferred_prompt_title_is_normalized_once(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "PWF".parse().unwrap(),
        "pwf",
        "/projects/pwf",
        "/tasks/pwf",
        false,
    )
    .await;
    let store = InMemoryStore::default().with_project_id("pwf", "PWF".parse().unwrap());
    let mut command = command(None);
    command.prompt = "fix # metadata".to_string();
    command.title = None;

    let added = execute(&command, &store, &pool, &FixedClock).await.unwrap();

    assert_eq!(added.title, "fix  metadata");
    assert_eq!(store.tasks("pwf")[0].title, "fix  metadata");
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_uses_clock_date_when_no_date_is_explicit(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "PWF".parse().unwrap(),
        "pwf",
        "/projects/pwf",
        "/tasks/pwf",
        false,
    )
    .await;
    let store = InMemoryStore::default().with_project_id("pwf", "PWF".parse().unwrap());
    let mut command = command(None);
    command.date = None;

    execute(&command, &store, &pool, &FixedClock).await.unwrap();

    assert_eq!(
        store.tasks("pwf")[0].created,
        Some(Timestamp::new("2026-07-26"))
    );
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_reports_created_section_only_when_region_absent(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "PWF".parse().unwrap(),
        "pwf",
        "/projects/pwf",
        "/tasks/pwf",
        false,
    )
    .await;
    let store = InMemoryStore::default().with_project_id("pwf", "PWF".parse().unwrap());

    let added = execute(&command(Some("Human")), &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(added.created_section.as_deref(), Some("Human"));
}

#[test]
fn plan_title_preserves_legacy_separator_whitespace_and_unicode_rules() {
    assert_eq!(
        plan_title(
            "DÉJÀ--  vu",
            "docs/plans/2026-01-01-MAÑANA__plan--cleanup.md"
        ),
        "déjà vu mañana cleanup"
    );
    assert_eq!(
        plan_title("foo---bar", "docs/plans/2026-01-01-plan.md"),
        "foo bar plan"
    );
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_plan_normalizes_yaml_significant_filename_title(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "PWF".parse().unwrap(),
        "pwf",
        "/projects/pwf",
        "/tasks/pwf",
        false,
    )
    .await;
    let store = InMemoryStore::default().with_project_id("pwf", "PWF".parse().unwrap());
    let mut command = command(None);
    command.continue_path = Some("docs/plans/2026-07-15-fix-#-metadata.md".parse().unwrap());

    let added = execute(&command, &store, &pool, &FixedClock).await.unwrap();

    assert_eq!(added.title, "pwf fix  metadata");
    assert_eq!(store.tasks("pwf")[0].title, "pwf fix  metadata");
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn add_does_not_report_created_section_for_existing_empty_region(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "PWF".parse().unwrap(),
        "pwf",
        "/projects/pwf",
        "/tasks/pwf",
        false,
    )
    .await;
    let store = InMemoryStore::default()
        .with_project_id("pwf", "PWF".parse().unwrap())
        .with_sections("pwf", &["Human"]);

    let added = execute(&command(Some("Human")), &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(added.created_section, None);
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn explicit_section_wins_over_human_shorthand(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "PWF".parse().unwrap(),
        "pwf",
        "/projects/pwf",
        "/tasks/pwf",
        false,
    )
    .await;
    let store = InMemoryStore::default().with_project_id("pwf", "PWF".parse().unwrap());
    let mut command = command(Some("future"));
    command.human = true;

    let added = execute(&command, &store, &pool, &FixedClock).await.unwrap();

    assert_eq!(added.created_section.as_deref(), Some("Future"));
}

#[sqlx::test(migrator = "crate::testing::MIGRATOR")]
async fn invalid_section_is_rejected_before_mutation(pool: sqlx::SqlitePool) {
    insert_project(
        &pool,
        "PWF".parse().unwrap(),
        "pwf",
        "/projects/pwf",
        "/tasks/pwf",
        false,
    )
    .await;
    let store = InMemoryStore::default().with_project_id("pwf", "PWF".parse().unwrap());
    let command = command(Some("someday"));

    let error = execute(&command, &store, &pool, &FixedClock)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        AddTaskError::InvalidSection { ref value } if value == "someday"
    ));
    assert!(store.tasks("pwf").is_empty());
}
