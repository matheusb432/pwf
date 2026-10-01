use pwf_application::task::{
    body_presets::MarkerSectionItemStyle, edit_task, edit_task::EditTaskError,
};
use pwf_models::task::{BlockedBy, EffortTier, TaskStatus, TaskTags, TaskTitle};
use pwf_wire::{
    collection_edit::CollectionEdit,
    patch_field::PatchField,
    set_field::SetField,
    task::{EditTask, EditTaskContent, RawTaskTags, TaskEdits, TaskRecord},
};

use crate::support::{
    FixedTaskBodyPresets, InMemoryStore, insert_project, stored_blocked_by, task_record,
    task_timestamp,
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
    run_with_presets(command, store, pool, &FixedTaskBodyPresets::default()).await
}

async fn run_with_presets(
    command: EditTask,
    store: &InMemoryStore,
    pool: &sqlx::SqlitePool,
    presets: &FixedTaskBodyPresets,
) -> Result<(), EditTaskError> {
    edit_task::execute(
        command,
        store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        presets,
    )
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
    let command = content_edit("FOO-0001", EditTaskContent::title(title("new title")));

    let error = run(command, &store, &pool).await.unwrap_err();

    assert!(matches!(
        error,
        EditTaskError::ClosedTask { ref id } if id.as_ref() == "FOO-0001"
    ));
    assert_eq!(store.tasks("foo-bar")[0].title, "tray gui");
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn body_replaces_title_and_every_marker_section(pool: sqlx::SqlitePool) {
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
async fn body_replacement_omits_explicit_empty_sections(pool: sqlx::SqlitePool) {
    register_project(&pool).await;
    let store = staged(vec![record(
        "FOO-0001",
        TaskStatus::Active,
        "## Goals\n\n- old",
    )]);
    let command = content_edit(
        "FOO-0001",
        EditTaskContent::replace_shorthand("New Title /c".into()),
    );

    run(command, &store, &pool).await.unwrap();

    let edited = &store.tasks("foo-bar")[0];
    assert_eq!(edited.title, "New Title");
    assert_eq!(edited.body, "## Goals\n");
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn body_replacement_uses_the_configured_preset_layout(pool: sqlx::SqlitePool) {
    register_project(&pool).await;
    let store = staged(vec![record(
        "FOO-0001",
        TaskStatus::Active,
        "## Goals\n\n- old",
    )]);
    let command = content_edit(
        "FOO-0001",
        EditTaskContent::replace_shorthand(
            "New Title /o first objective / second objective /v tests pass".into(),
        ),
    );

    run_with_presets(
        command,
        &store,
        &pool,
        &FixedTaskBodyPresets::selecting(&[
            ("/o", "Objectives", 3, MarkerSectionItemStyle::Numbered),
            ("/v", "Verification", 4, MarkerSectionItemStyle::Paragraph),
        ]),
    )
    .await
    .unwrap();

    let edited = &store.tasks("foo-bar")[0];
    assert_eq!(edited.title, "New Title");
    assert_eq!(
        edited.body,
        "### Objectives\n\n1. first objective\n2. second objective\n\n#### Verification\n\ntests pass"
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn body_replacement_rejects_a_configured_marker_before_the_title(pool: sqlx::SqlitePool) {
    register_project(&pool).await;
    let store = staged(vec![record(
        "FOO-0001",
        TaskStatus::Active,
        "## Goals\n\n- old",
    )]);
    let command = content_edit(
        "FOO-0001",
        EditTaskContent::replace_shorthand("/o no title".into()),
    );

    let error = run_with_presets(
        command,
        &store,
        &pool,
        &FixedTaskBodyPresets::selecting(&[(
            "/o",
            "Objectives",
            2,
            MarkerSectionItemStyle::Bullet,
        )]),
    )
    .await
    .unwrap_err();

    assert!(matches!(error, EditTaskError::InvalidTitle(_)));
    assert_eq!(store.tasks("foo-bar")[0].body, "## Goals\n\n- old");
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn append_uses_the_task_project_preset_and_existing_item_style(pool: sqlx::SqlitePool) {
    register_project(&pool).await;
    let store = staged(vec![record(
        "FOO-0001",
        TaskStatus::Active,
        "## Goals\n\n1. first\n\n## Notes\n\nkeep me",
    )]);
    let command = content_edit(
        "FOO-0001",
        EditTaskContent::append_shorthand(SetField::NoAction, "second /c first / second".into())
            .unwrap(),
    );

    run_with_presets(
        command,
        &store,
        &pool,
        &FixedTaskBodyPresets::for_project(
            "FOO",
            &[
                ("/g", "Goals", 3, MarkerSectionItemStyle::Bullet),
                ("/c", "Context", 4, MarkerSectionItemStyle::Paragraph),
            ],
        ),
    )
    .await
    .unwrap();

    assert_eq!(
        store.tasks("foo-bar")[0].body,
        "## Goals\n\n1. first\n2. second\n\n## Notes\n\nkeep me\n\n#### Context\n\nfirst\n\nsecond\n"
    );
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
async fn malformed_dependencies_only_prevent_appending_to_them(pool: sqlx::SqlitePool) {
    register_project(&pool).await;
    let malformed = pwf_wire::task::StoredBlockedBy::Malformed {
        raw: "broken links".into(),
        reason: "expected a sequence".into(),
    };
    let blockers = crate::support::blocked_by(&["FOO-0002"]);
    for (dependency_edit, expected) in [
        (CollectionEdit::Unchanged, malformed.clone()),
        (
            CollectionEdit::Clear,
            pwf_wire::task::StoredBlockedBy::Absent,
        ),
        (
            CollectionEdit::Replace(blockers.clone()),
            stored_blocked_by(&["FOO-0002"]),
        ),
        (CollectionEdit::Append(blockers), malformed.clone()),
    ] {
        let appends = matches!(dependency_edit, CollectionEdit::Append(_));
        let store = staged(vec![
            TaskRecord {
                blocked_by: malformed.clone(),
                ..task_record("FOO-0001")
            },
            task_record("FOO-0002"),
        ]);
        let command = edit(
            "FOO-0001",
            SetField::Set(EditTaskContent::title(title("repaired"))),
            dependency_edit,
            PatchField::NoAction,
            CollectionEdit::Unchanged,
        );
        let result = run(command, &store, &pool).await;
        if appends {
            assert!(matches!(
                result,
                Err(EditTaskError::MalformedBlockedBy { .. })
            ));
        } else {
            result.unwrap();
            assert_eq!(store.tasks("foo-bar")[0].title, "repaired");
        }
        assert_eq!(store.tasks("foo-bar")[0].blocked_by, expected);
    }
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
async fn edit_rejects_a_missing_task_file(pool: sqlx::SqlitePool) {
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
    assert_eq!(
        store.tasks("foo-bar"),
        Vec::<pwf_wire::task::TaskRecord>::new()
    );
}
