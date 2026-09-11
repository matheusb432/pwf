use pwf_application::task::{edit_task, edit_task::EditTaskError};
use pwf_models::task::{BlockedBy, EffortTier, TaskStatus, TaskTags, TaskTitle};
use pwf_wire::{
    collection_edit::CollectionEdit,
    patch_field::PatchField,
    set_field::SetField,
    task::{
        EditTask, EditTaskContent, RawTaskTags, TaskEdits, TaskLane, TaskLaneEdits, TaskLanes,
        TaskRecord,
    },
};

use crate::support::{
    InMemoryStore, insert_project, stored_blocked_by, task_record, task_timestamp,
};

fn record(id: &str, status: TaskStatus, body: &str) -> TaskRecord {
    TaskRecord {
        status,
        completed_at: (status != TaskStatus::Active)
            .then(|| task_timestamp("2026-06-20T12:34:56Z")),
        body: body.to_string(),
        source: format!("---\nstatus: {status}\n---\n{body}"),
        ..task_record(id)
    }
}

fn staged(records: Vec<TaskRecord>) -> InMemoryStore {
    InMemoryStore::default()
        .with_project_id("foo-bar", "FOO")
        .with_project("foo-bar", records)
}

fn edit(
    id: &str,
    content: SetField<EditTaskContent>,
    blocked_by: CollectionEdit<BlockedBy>,
    effort: PatchField<EffortTier>,
    tags: CollectionEdit<TaskTags>,
) -> EditTask {
    EditTask {
        id: id.parse().unwrap(),
        edits: TaskEdits::try_new(content, blocked_by, effort, tags, PatchField::NoAction).unwrap(),
        expected_revision: None,
    }
}

fn content_edit(id: &str, content: EditTaskContent) -> EditTask {
    edit(
        id,
        SetField::Set(content),
        CollectionEdit::Unchanged,
        PatchField::NoAction,
        CollectionEdit::Unchanged,
    )
}

fn title(raw: &str) -> TaskTitle {
    TaskTitle::try_new(raw).unwrap()
}

fn tags(raw: &str) -> TaskTags {
    TaskTags::from_inputs(&[raw.parse().unwrap()]).unwrap()
}

async fn run(
    command: EditTask,
    store: &InMemoryStore,
    pool: &sqlx::SqlitePool,
) -> Result<(), EditTaskError> {
    edit_task::execute(command, store, pool)
        .await
        .map(|result| result.outcome)
}

async fn register_project(pool: &sqlx::SqlitePool) {
    insert_project(pool, "FOO", "foo-bar", "/projects/foo", "/tasks/foo", false).await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn edit_rejects_closed_tasks_before_content_changes(pool: sqlx::SqlitePool) {
    register_project(&pool).await;
    let store = staged(vec![record(
        "FOO-0001",
        TaskStatus::Done,
        "## Goals\n\n- keep this",
    )]);
    let command = content_edit(
        "FOO-0001",
        EditTaskContent::structured(SetField::Set(title("new title")), TaskLaneEdits::default())
            .unwrap(),
    );

    let error = run(command, &store, &pool).await.unwrap_err();

    assert!(matches!(
        error,
        EditTaskError::ClosedTask { ref id } if id.as_ref() == "FOO-0001"
    ));
    assert_eq!(store.tasks("foo-bar")[0].title, "tray gui");
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn prompt_replaces_title_and_every_lane(pool: sqlx::SqlitePool) {
    register_project(&pool).await;
    let store = staged(vec![record(
        "FOO-0001",
        TaskStatus::Active,
        "## Goals\n\n- old\n\n## Context\n\n- old context",
    )]);
    let command = content_edit(
        "FOO-0001",
        EditTaskContent::replace_shorthand("New: Title; #123 / new goal /d complete".into()),
    );

    run(command, &store, &pool).await.unwrap();

    let edited = &store.tasks("foo-bar")[0];
    assert_eq!(edited.title, "New: Title; #123");
    assert_eq!(
        edited.body,
        "## Goals\n\n- new goal\n\n## Done When\n\n- complete"
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn prompt_replacement_uses_runtime_markers_and_headers(pool: sqlx::SqlitePool) {
    register_project(&pool).await;
    sqlx::query(
        "UPDATE task_prompt_lanes SET marker = '/o', header = 'Objectives' WHERE lane = 'goals'",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE task_prompt_lanes SET marker = '/v', header = 'Verification' WHERE lane = 'done_when'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let store = staged(vec![record(
        "FOO-0001",
        TaskStatus::Active,
        "## Goals\n\n- old",
    )]);
    let command = content_edit(
        "FOO-0001",
        EditTaskContent::replace_shorthand("New Title /o new objective /v tests pass".into()),
    );

    run(command, &store, &pool).await.unwrap();

    let edited = &store.tasks("foo-bar")[0];
    assert_eq!(edited.title, "New Title");
    assert_eq!(
        edited.body,
        "## Objectives\n\n- new objective\n\n## Verification\n\n- tests pass"
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn prompt_replacement_rejects_a_runtime_marker_before_the_title(pool: sqlx::SqlitePool) {
    register_project(&pool).await;
    sqlx::query("UPDATE task_prompt_lanes SET marker = '/o' WHERE lane = 'goals'")
        .execute(&pool)
        .await
        .unwrap();
    let store = staged(vec![record(
        "FOO-0001",
        TaskStatus::Active,
        "## Goals\n\n- old",
    )]);
    let command = content_edit(
        "FOO-0001",
        EditTaskContent::replace_shorthand("/o no title".into()),
    );

    let error = run(command, &store, &pool).await.unwrap_err();

    assert!(matches!(error, EditTaskError::InvalidTitle(_)));
    assert_eq!(store.tasks("foo-bar")[0].body, "## Goals\n\n- old");
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn structured_content_replaces_lanes_and_preserves_unrelated_markdown(
    pool: sqlx::SqlitePool,
) {
    register_project(&pool).await;
    let store = staged(vec![record(
        "FOO-0001",
        TaskStatus::Active,
        "## Goals\n\n- old\n\n## Notes\n\nkeep me\n\n## Done When\n\n- old outcome",
    )]);
    let additions = TaskLanes::try_new(
        vec!["new /c literal".to_string()],
        vec!["new context".to_string()],
        Vec::new(),
        vec!["new outcome".to_string()],
    )
    .unwrap();
    let command = content_edit(
        "FOO-0001",
        EditTaskContent::structured(
            SetField::NoAction,
            TaskLaneEdits::new(
                additions,
                [TaskLane::Goal, TaskLane::Context, TaskLane::DoneWhen],
            ),
        )
        .unwrap(),
    );

    run(command, &store, &pool).await.unwrap();

    assert_eq!(
        store.tasks("foo-bar")[0].body,
        "## Goals\n\n- new /c literal\n\n## Notes\n\nkeep me\n\n## Context\n\n- new context\n\n## Done When\n\n- new outcome"
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn structured_content_rejects_duplicate_lane_headings(pool: sqlx::SqlitePool) {
    register_project(&pool).await;
    let body = "## Goals\n\n- one\n\n## Goals\n\n- two";
    let store = staged(vec![record("FOO-0001", TaskStatus::Active, body)]);
    let command = content_edit(
        "FOO-0001",
        EditTaskContent::structured(
            SetField::NoAction,
            TaskLaneEdits::new(TaskLanes::default(), [TaskLane::Goal]),
        )
        .unwrap(),
    );

    let error = run(command, &store, &pool).await.unwrap_err();

    assert!(matches!(
        error,
        EditTaskError::AmbiguousLanes { ref header } if header == "## Goals"
    ));
    assert_eq!(store.tasks("foo-bar")[0].body, body);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn append_can_replace_the_title_without_reparsing_it(pool: sqlx::SqlitePool) {
    register_project(&pool).await;
    let store = staged(vec![record(
        "FOO-0001",
        TaskStatus::Active,
        "## Goals\n\n- old",
    )]);
    let command = content_edit(
        "FOO-0001",
        EditTaskContent::append_shorthand(
            SetField::Set(title("renamed")),
            "additional /c context".into(),
        )
        .unwrap(),
    );

    run(command, &store, &pool).await.unwrap();

    let edited = &store.tasks("foo-bar")[0];
    assert_eq!(edited.title, "renamed");
    assert_eq!(
        edited.body,
        "## Goals\n\n- old\n- additional\n\n## Context\n\n- context\n"
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn replacing_malformed_tags_does_not_require_unrelated_metadata_to_parse(
    pool: sqlx::SqlitePool,
) {
    register_project(&pool).await;
    let target = TaskRecord {
        tags: Some(RawTaskTags::new("broken tags")),
        effort: Some("extreme".to_string()),
        ..record("FOO-0001", TaskStatus::Active, "authored body")
    };
    let store = staged(vec![target]);
    let command = edit(
        "FOO-0001",
        SetField::NoAction,
        CollectionEdit::Unchanged,
        PatchField::NoAction,
        CollectionEdit::Replace(tags("rust")),
    );
    run(command, &store, &pool).await.unwrap();
    let records = store.tasks("foo-bar");
    assert_eq!(records[0].tags.as_ref().map(AsRef::as_ref), Some("[rust]"));
    assert_eq!(records[0].effort.as_deref(), Some("extreme"));
    assert_eq!(records[0].body, "authored body");
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn remove_then_add_replaces_tags_blocked_by_and_effort(pool: sqlx::SqlitePool) {
    register_project(&pool).await;
    let target = TaskRecord {
        tags: Some(RawTaskTags::new("[old]")),
        blocked_by: stored_blocked_by(&["FOO-0001"]),
        effort: Some("medium".to_string()),
        ..record("FOO-0002", TaskStatus::Active, "## Goals\n")
    };
    let store = staged(vec![record("FOO-0001", TaskStatus::Done, "body"), target]);
    let command = edit(
        "FOO-0002",
        SetField::NoAction,
        CollectionEdit::Replace(crate::support::blocked_by(&["FOO-0001"])),
        PatchField::Set(EffortTier::High),
        CollectionEdit::Replace(tags("new-tag")),
    );

    run(command, &store, &pool).await.unwrap();

    let edited = store
        .tasks("foo-bar")
        .into_iter()
        .find(|task| task.id.as_ref() == "FOO-0002")
        .unwrap();
    assert_eq!(edited.tags.as_ref().map(AsRef::as_ref), Some("[new_tag]"));
    assert_eq!(
        edited
            .blocked_by
            .valid()
            .map(|value| value.iter().map(AsRef::as_ref).collect::<Vec<_>>()),
        Some(vec!["FOO-0001"])
    );
    assert_eq!(edited.effort.as_deref(), Some("high"));
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn blocked_by_edit_rejects_a_multihop_cycle_without_mutating_the_task(
    pool: sqlx::SqlitePool,
) {
    register_project(&pool).await;
    let first = TaskRecord {
        blocked_by: stored_blocked_by(&["FOO-0002"]),
        ..record("FOO-0001", TaskStatus::Done, "first")
    };
    let second = TaskRecord {
        blocked_by: stored_blocked_by(&["FOO-0003"]),
        ..record("FOO-0002", TaskStatus::Cancelled, "second")
    };
    let target = record("FOO-0003", TaskStatus::Active, "target");
    let store = staged(vec![first, second, target]);
    let before = store.tasks("foo-bar");
    let command = edit(
        "FOO-0003",
        SetField::NoAction,
        CollectionEdit::Append(crate::support::blocked_by(&["FOO-0001"])),
        PatchField::NoAction,
        CollectionEdit::Unchanged,
    );

    let error = run(command, &store, &pool).await.unwrap_err();

    assert_eq!(
        error.to_string(),
        "blocked_by cycle: FOO-0003 -> FOO-0001 -> FOO-0002 -> FOO-0003"
    );
    assert!(matches!(
        &error,
        EditTaskError::BlockedByCycle { path }
            if path.iter().map(AsRef::as_ref).collect::<Vec<_>>()
                == ["FOO-0003", "FOO-0001", "FOO-0002", "FOO-0003"]
    ));
    assert_eq!(store.tasks("foo-bar"), before);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn remove_effort_clears_existing_effort(pool: sqlx::SqlitePool) {
    register_project(&pool).await;
    let store = staged(vec![TaskRecord {
        effort: Some("medium".to_string()),
        ..record("FOO-0001", TaskStatus::Active, "## Goals\n")
    }]);
    let command = edit(
        "FOO-0001",
        SetField::NoAction,
        CollectionEdit::Unchanged,
        PatchField::Clear,
        CollectionEdit::Unchanged,
    );

    run(command, &store, &pool).await.unwrap();

    assert_eq!(store.tasks("foo-bar")[0].effort, None);
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn metadata_edit_does_not_validate_an_unreturned_persisted_title(pool: sqlx::SqlitePool) {
    register_project(&pool).await;
    let store = staged(vec![TaskRecord {
        title: "x".repeat(201),
        ..record("FOO-0001", TaskStatus::Active, "## Goals\n")
    }]);
    let command = edit(
        "FOO-0001",
        SetField::NoAction,
        CollectionEdit::Unchanged,
        PatchField::Set(EffortTier::High),
        CollectionEdit::Unchanged,
    );

    run(command, &store, &pool).await.unwrap();

    assert_eq!(store.tasks("foo-bar")[0].effort.as_deref(), Some("high"));
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn edit_rejects_a_missing_task_note(pool: sqlx::SqlitePool) {
    register_project(&pool).await;
    let store = staged(Vec::new());
    let command = edit(
        "FOO-0001",
        SetField::NoAction,
        CollectionEdit::Unchanged,
        PatchField::Set(EffortTier::High),
        CollectionEdit::Unchanged,
    );

    let error = run(command, &store, &pool).await.unwrap_err();

    assert!(matches!(
        error,
        EditTaskError::TaskNotFound { ref id } if id.as_ref() == "FOO-0001"
    ));
    assert!(store.tasks("foo-bar").is_empty());
}
