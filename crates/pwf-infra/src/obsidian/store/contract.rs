use std::{assert_matches, fmt::Write as _, path::Path};

use pwf_application::ports::task_vault::{
    ExpectedTaskRevision, NewTask, NullablePatch, TaskMutationError, TaskPatch, TaskVault,
    TaskWrite, TaskWriteSet,
};
use pwf_models::{
    project::{
        HomeDirectory, Project, ProjectId, ProjectName, ProjectSource, ProjectSourceKind,
        ProjectSourceValue, ProjectTasks, ProjectTasksKind, ProjectTasksPath,
    },
    task::{
        BlockedBy, EffortTier, PriorityTier, Tag, TaskId, TaskStatus, TaskTags, TaskTimestamp,
        TaskTitle,
    },
};
use pwf_wire::{
    set_field::SetField,
    task::{StoredBlockedBy, TaskRecord},
};

use super::{ObsidianStore, ObsidianStoreError};
use crate::file_transaction::content_revision;

const S: &str = "\n\n";

fn path_str(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn task_timestamp(raw: &str) -> TaskTimestamp {
    raw.parse().unwrap()
}

fn project(id: &str, title: &str, tasks_path: &Path) -> Project {
    Project {
        obsidian_vault: None,
        id: ProjectId::try_new(id.to_ascii_uppercase()).unwrap(),
        title: ProjectName::try_new(title).unwrap(),
        source: Some(ProjectSource::new(
            ProjectSourceKind::Directory,
            ProjectSourceValue::try_new(format!("/projects/{title}")).unwrap(),
        )),
        tasks: ProjectTasks::new(
            ProjectTasksKind::Directory,
            ProjectTasksPath::try_new(path_str(tasks_path)).unwrap(),
        ),
        created_at: "2026-07-25T00:00:00.000Z".parse().unwrap(),
        is_paused: false,
        snapshot_enabled: false,
    }
}

fn foo_project(store: &ObsidianStore) -> Project {
    project("FOO", "foo", store.home.as_path())
}

#[test]
fn task_records_ignore_project_page_contents() {
    let directory = tempfile::tempdir().unwrap();
    let store = store_with_index_identity(directory.path());
    let project = foo_project(&store);
    std::fs::write(
        directory.path().join("FOO-0001.md"),
        "---\nid: FOO-0001\ntitle: Task\nstatus: active\n---\n\nDo work\n",
    )
    .unwrap();
    for page in [
        "---\nid: foo\ntitle: foo\n---\n\n## Later\n- [x] [[FOO-0001]]\n- [ ] [[FOO-0002]]\n",
        "---\ninvalid: [\n---\n",
    ] {
        std::fs::write(directory.path().join("foo.md"), page).unwrap();
        let records = TaskVault::list_tasks(&store, &project).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, TaskStatus::Active);
        assert!(
            TaskVault::get_task_record(&store, &project, &TaskId::try_new("FOO-0002").unwrap())
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn explicit_task_path_is_the_complete_project_directory() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let tasks_path = temporary_directory.path().join("custom/tasks");
    std::fs::create_dir_all(&tasks_path).unwrap();
    std::fs::write(
        tasks_path.join("foo.md"),
        "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0001]]\n",
    )
    .unwrap();
    write_note(
        &tasks_path.join("FOO-0001.md"),
        "exact path",
        "2026-07-25",
        None,
        None,
        None,
        "body",
    );
    let store = ObsidianStore::new(HomeDirectory::new(tasks_path.clone()));

    let record = get_record(&store, "FOO-0001").unwrap();

    assert_eq!(record.locator.as_path(), tasks_path.join("FOO-0001.md"));
    assert!(!tasks_path.join("foo").exists());
}

#[test]
fn list_returns_empty_when_project_directory_is_missing() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let tasks_path = temporary_directory.path().join("missing");
    let store = store_for_tasks(&tasks_path);
    let project = foo_project(&store);

    let records = TaskVault::list_tasks(&store, &project).unwrap();
    assert_eq!(
        TaskVault::list_task_summaries(&store, &project).unwrap(),
        records
            .iter()
            .cloned()
            .map(pwf_application::ports::task_vault::TaskSummaryRecord::from)
            .collect::<Vec<_>>()
    );

    assert!(records.is_empty());
    assert!(!tasks_path.exists());
}

#[test]
fn summaries_read_frontmatter_without_decoding_the_body() {
    let directory = tempfile::tempdir().unwrap();
    let store = store_with_index_identity(directory.path());
    let mut source = b"---\nid: FOO-0001\ntitle: 'quoted: title'\nstatus: active\n---\n".to_vec();
    source.extend([0xff; 8192]);
    std::fs::write(directory.path().join("descriptive-name.md"), source).unwrap();
    let project = foo_project(&store);
    let summaries = TaskVault::list_task_summaries(&store, &project).unwrap();
    assert_eq!(summaries[0].id.as_ref(), "FOO-0001");
    assert_eq!(summaries[0].title, "quoted: title");
    assert!(TaskVault::list_tasks(&store, &project).is_err());
}

#[test]
fn both_list_projections_reject_duplicate_frontmatter_ids() {
    let directory = tempfile::tempdir().unwrap();
    let store = store_with_index_identity(directory.path());
    for name in ["first.md", "second.md"] {
        std::fs::write(
            directory.path().join(name),
            "---\nid: FOO-0001\ntitle: duplicate\n---\nbody\n",
        )
        .unwrap();
    }
    let project = foo_project(&store);
    for error in [
        TaskVault::list_tasks(&store, &project).unwrap_err(),
        TaskVault::list_task_summaries(&store, &project).unwrap_err(),
    ] {
        assert_matches!(error, ObsidianStoreError::DuplicateTaskId { id, paths } if id.as_ref() == "FOO-0001" && paths.len() == 2);
    }
}

#[test]
fn list_ignores_markdown_without_task_id() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let tasks_path = temporary_directory.path().join("foo");
    std::fs::create_dir_all(&tasks_path).unwrap();
    std::fs::write(
        tasks_path.join("foo.md"),
        "---\nid: foo\ntitle: foo\n---\n\n",
    )
    .unwrap();
    let store = store_for_tasks(&tasks_path);
    write_note(
        &tasks_path.join("FOO-0001.md"),
        "declared task",
        "2026-07-25",
        None,
        None,
        None,
        "body",
    );
    std::fs::write(
        tasks_path.join("FOO-0002.plan.md"),
        "---\ntitle: supporting plan\n---\n\nplan\n",
    )
    .unwrap();
    std::fs::write(tasks_path.join("supporting-note.md"), "# Supporting note\n").unwrap();
    let project = foo_project(&store);

    let records = TaskVault::list_tasks(&store, &project).unwrap();
    assert_eq!(
        TaskVault::list_task_summaries(&store, &project).unwrap(),
        records
            .iter()
            .cloned()
            .map(pwf_application::ports::task_vault::TaskSummaryRecord::from)
            .collect::<Vec<_>>()
    );

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, TaskId::try_new("FOO-0001").unwrap());
}

#[test]
fn explicit_projects_support_unrelated_task_parents() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let first_tasks = temporary_directory.path().join("one/tasks-a");
    let second_tasks = temporary_directory.path().join("elsewhere/tasks-b");
    for (tasks_path, id, title, task_id) in [
        (&first_tasks, "aaa", "alpha", "AAA-0001"),
        (&second_tasks, "bbb", "beta", "BBB-0001"),
    ] {
        std::fs::create_dir_all(tasks_path).unwrap();
        std::fs::write(
            tasks_path.join(format!("{title}.md")),
            format!("---\nid: {id}\ntitle: {title}\n---\n\n- [ ] [[{task_id}]]\n"),
        )
        .unwrap();
        std::fs::write(
            tasks_path.join(format!("{task_id}.md")),
            format!("---\nid: {task_id}\ntitle: Task\n---\nbody\n"),
        )
        .unwrap();
    }
    let store = ObsidianStore::new(HomeDirectory::new(temporary_directory.path().to_path_buf()));

    for (title, expected_id) in [("alpha", "AAA-0001"), ("beta", "BBB-0001")] {
        let tasks_path = if title == "alpha" {
            &first_tasks
        } else {
            &second_tasks
        };
        let project_name = project(&expected_id[..3], title, tasks_path);
        let records = TaskVault::list_tasks(&store, &project_name).unwrap();
        assert_eq!(records[0].id.as_ref(), expected_id);
    }
}

#[test]
fn generic_list_reports_an_unreadable_task_path() {
    let temp = tempfile::tempdir().unwrap();
    let project_dir = temp.path().join("foo");
    std::fs::create_dir_all(project_dir.join("FOO-0001.md")).unwrap();
    std::fs::write(
        project_dir.join("foo.md"),
        "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0001]]\n",
    )
    .unwrap();
    let store = store_for_tasks(&project_dir);
    let project = foo_project(&store);

    let error = TaskVault::list_tasks(&store, &project).unwrap_err();

    assert_matches!(error, ObsidianStoreError::ReadTaskFile { .. });
}

#[test]
fn generic_read_retains_raw_tags_frontmatter() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("foo.md"), "- [ ] [[FOO-0001]]\n").unwrap();
    write_note(
        &project_dir.join("FOO-0001.md"),
        "tagged",
        "2026-07-01",
        None,
        None,
        Some("[SQLite, malformed-but-displayable]"),
        "body",
    );
    let store = store_with_index_identity(&notes_dir.join("foo"));

    assert_eq!(
        get_record(&store, "FOO-0001")
            .unwrap()
            .tags
            .as_ref()
            .map(AsRef::as_ref),
        Some("[SQLite, malformed-but-displayable]")
    );
}

#[test]
fn generic_read_uses_yaml_decoded_title() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("foo.md"), "- [ ] [[FOO-0001]]\n").unwrap();
    std::fs::write(
        project_dir.join("FOO-0001.md"),
        "---\nid: FOO-0001\nstatus: active\ntitle: \"adapter: preserve identity\"\nproject: foo\ncreated: 2026-07-12\n---\n\nbody\n",
    )
    .unwrap();
    let store = store_with_index_identity(&notes_dir.join("foo"));

    let record = get_record(&store, "FOO-0001").unwrap();

    assert_eq!(record.title, "adapter: preserve identity");
}

fn get_record(store: &ObsidianStore, id: &str) -> Option<TaskRecord> {
    let project = foo_project(store);
    TaskVault::get_task_record(store, &project, &TaskId::try_new(id).unwrap()).unwrap()
}

#[test]
fn get_resolves_frontmatter_id_to_descriptive_filename_locator() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("foo.md"), "- [ ] [[FOO-0001]]\n").unwrap();
    std::fs::write(
        project_dir.join("descriptive-name.md"),
        "---\nid: FOO-0001\nstatus: active\ntitle: descriptive\nproject: foo\ncreated: 2026-07-12\n---\n\nbody\n",
    )
    .unwrap();
    let store = store_with_index_identity(&notes_dir.join("foo"));

    let record = get_record(&store, "FOO-0001").unwrap();

    assert_eq!(
        record.locator.as_path(),
        project_dir.join("descriptive-name.md")
    );
}

#[test]
fn get_rejects_duplicate_frontmatter_ids() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("foo.md"), "- [ ] [[FOO-0001]]\n").unwrap();
    for (filename, id) in [
        ("a.md", "FOO-0001"),
        ("b.md", "FOO-0002"),
        ("c.md", "FOO-0001"),
    ] {
        std::fs::write(
            project_dir.join(filename),
            format!("---\nid: {id}\nstatus: active\ntitle: task\nproject: foo\ncreated: 2026-07-12\n---\n\nbody\n"),
        )
        .unwrap();
    }
    let store = store_with_index_identity(&notes_dir.join("foo"));
    let project = foo_project(&store);

    let error = TaskVault::get_task_record(&store, &project, &TaskId::try_new("FOO-0001").unwrap())
        .unwrap_err();

    assert_matches!(
        error,
        ObsidianStoreError::DuplicateTaskId { ref id, .. } if id.as_ref() == "FOO-0001"
    );
}

fn write_note(
    path: &Path,
    title: &str,
    created_date: &str,
    blocked_by: Option<&str>,
    effort: Option<&str>,
    tags: Option<&str>,
    body: &str,
) {
    let id = path.file_stem().and_then(|stem| stem.to_str()).unwrap();
    let mut note = format!(
        "---\nid: {id}\nstatus: active\ntitle: {title}\nproject: foo\ncreated_at: {created_date}T00:00:00Z\n"
    );
    if let Some(blocked_by) = blocked_by {
        let _ = writeln!(note, "blocked_by: {blocked_by}");
    }
    if let Some(effort) = effort {
        let _ = writeln!(note, "effort: {effort}");
    }
    if let Some(tags) = tags {
        let _ = writeln!(note, "tags: {tags}");
    }
    let _ = write!(note, "---\n\n{body}\n");
    std::fs::write(path, note).unwrap();
}

fn generic_add(store: &ObsidianStore, new: NewTask) -> Result<TaskRecord, ObsidianStoreError> {
    insert_next(store, &foo_project(store), new)
}

fn insert_next(
    store: &ObsidianStore,
    project: &Project,
    new: NewTask,
) -> Result<TaskRecord, ObsidianStoreError> {
    let id = TaskVault::next_task_id(store, project)?;
    TaskVault::insert_task(store, project, &id, new)
}

fn new_task(body: &str, title: &str) -> NewTask {
    NewTask {
        body: body.to_string().into(),
        title: TaskTitle::try_new(title).unwrap(),
        created_at: task_timestamp("2026-07-07T09:34:56-03:00"),
        blocked_by: None,
        effort: None,
        priority: None,
        tags: None,
    }
}

#[test]
fn generic_add_creates_note_and_preserves_project_page() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let store = store_with_index_identity(&notes_dir.join("foo"));

    let record = generic_add(
        &store,
        NewTask {
            blocked_by: Some(
                pwf_models::task::BlockedBy::try_new(["FOO-0001".parse().unwrap()]).unwrap(),
            ),
            effort: Some(EffortTier::Medium),
            priority: Some(PriorityTier::Highest),
            ..new_task("## Goals\n\n## Done When\n\n- tests pass", "ship adapter")
        },
    )
    .unwrap();

    assert_eq!(record.id, TaskId::try_new("FOO-0001").unwrap());
    assert_eq!(record.title, "ship adapter");
    let note = std::fs::read_to_string(notes_dir.join("foo/FOO-0001.md")).unwrap();
    assert!(
        note.starts_with("---\nid: FOO-0001\nstatus: active\n"),
        "{note}"
    );
    assert!(note.contains("status: active"), "{note}");
    assert!(note.contains("title: ship adapter"), "{note}");
    assert!(
        note.contains("created_at: 2026-07-07T09:34:56-03:00"),
        "{note}"
    );
    assert!(note.contains("blocked_by: [\"[[FOO-0001]]\"]"), "{note}");
    assert!(note.contains("effort: medium"), "{note}");
    assert!(note.contains("priority: highest"), "{note}");
    let expected_body = format!("## Goals{S}## Done When{S}- tests pass");
    assert!(note.contains(&expected_body), "{note}");
    let index = std::fs::read_to_string(notes_dir.join("foo/foo.md")).unwrap();
    assert_eq!(index, "---\nid: foo\ntitle: foo\n---\n\n");
}

#[test]
fn generic_add_writes_tags_and_omits_absent_tags() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let store = store_with_index_identity(&notes_dir.join("foo"));
    let tags = tags(&["sqlite", "csharp_export"]);
    let tagged = generic_add(
        &store,
        NewTask {
            tags: Some(tags),
            ..new_task("tagged task", "tagged task")
        },
    )
    .unwrap();
    let note = std::fs::read_to_string(tagged.locator.as_path()).unwrap();
    assert!(note.contains("tags: [sqlite, csharp_export]\n"), "{note}");

    let untagged = generic_add(&store, new_task("untagged task", "untagged task")).unwrap();
    let note = std::fs::read_to_string(untagged.locator.as_path()).unwrap();
    assert!(!note.contains("tags:"), "{note}");
}

#[test]
fn generic_insert_allocates_after_greatest_frontmatter_id() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("foo.md"),
        "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0009]]\n",
    )
    .unwrap();
    std::fs::write(
        project_dir.join("descriptive.md"),
        "---\nid: FOO-0009\nstatus: active\ntitle: existing\nproject: foo\ncreated: 2026-07-12\n---\n\nbody\n",
    )
    .unwrap();
    std::fs::write(
        project_dir.join("FOO-0099.md"),
        "---\ntype: note\n---\n\nnote\n",
    )
    .unwrap();
    let store = store_with_index_identity(&notes_dir.join("foo"));
    let project = foo_project(&store);

    let record = insert_next(&store, &project, new_task("next task", "next task")).unwrap();

    assert_eq!(record.id, TaskId::try_new("FOO-0010").unwrap());
}

#[test]
fn generic_insert_reports_exhausted_task_id_sequence() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("foo.md"),
        "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-9999]]\n",
    )
    .unwrap();
    write_note(
        &project_dir.join("FOO-9999.md"),
        "last task",
        "2026-07-01",
        None,
        None,
        None,
        "body",
    );
    let store = store_with_index_identity(&project_dir);
    let project = foo_project(&store);

    let error = insert_next(&store, &project, new_task("next task", "next task")).unwrap_err();

    assert_matches!(
        error,
        ObsidianStoreError::TaskIdSequenceExhausted { ref project_id }
            if project_id.as_ref() == "FOO"
    );
}

/// Applies a tags-only patch; `Some(tags)` sets the field and `None` clears it.
fn apply_tag_patch(store: &ObsidianStore, tags: Option<TaskTags>) {
    let project = foo_project(store);
    let id = TaskId::try_new("FOO-0001").unwrap();
    let patch = TaskPatch {
        tags: tags.map_or(NullablePatch::Clear, NullablePatch::Set),
        ..Default::default()
    };
    commit_for_task(
        store,
        &project,
        &id,
        vec![TaskWrite::Patch {
            id: id.clone(),
            patch,
        }],
    );
}

fn commit_for_task(store: &ObsidianStore, project: &Project, id: &TaskId, writes: Vec<TaskWrite>) {
    let record = TaskVault::get_task_record(store, project, id)
        .unwrap()
        .unwrap();
    let writes = TaskWriteSet::try_new(
        vec![ExpectedTaskRevision {
            id: id.clone(),
            revision: record.revision,
        }],
        writes,
    )
    .unwrap();
    TaskVault::commit_task_writes(store, project, writes).unwrap();
}

fn sqlite_tags() -> TaskTags {
    tags(&["sqlite"])
}

fn tags(values: &[&str]) -> TaskTags {
    TaskTags::try_new(
        values
            .iter()
            .map(|value| Tag::try_from(*value).unwrap())
            .collect(),
    )
    .unwrap()
}

#[test]
fn generic_update_sets_frontmatter_tags_without_rewriting_body_tags_line() {
    let body = "tags: body-only value\nkeep this body byte-identical";
    let StagedOpenTask {
        _temp,
        store,
        task_path,
    } = staged_open_task(None, body);

    apply_tag_patch(&store, Some(sqlite_tags()));

    assert_eq!(
        std::fs::read_to_string(task_path).unwrap(),
        concat!(
            "---\n",
            "id: FOO-0001\n",
            "status: active\n",
            "title: tagged\n",
            "project: foo\n",
            "created_at: 2026-07-01T00:00:00Z\n",
            "tags: [sqlite]\n",
            "---\n\n",
            "tags: body-only value\n",
            "keep this body byte-identical\n",
        )
    );
}

#[test]
fn generic_update_writes_a_deduplicated_tag_value() {
    let StagedOpenTask {
        _temp,
        store,
        task_path,
    } = staged_open_task_with_tags("[sqlite, godot]");
    let merged = tags(&["sqlite", "godot", "csharp_export"]);

    apply_tag_patch(&store, Some(merged));

    let note = std::fs::read_to_string(task_path).unwrap();
    assert!(
        note.contains("tags: [sqlite, godot, csharp_export]\n"),
        "{note}"
    );
    assert_eq!(note.matches("tags:").count(), 1);
}

#[test]
fn generic_update_clear_preserves_body_tags_line_when_frontmatter_has_no_tags() {
    let body = "tags: body-only value\nkeep this body byte-identical";
    let StagedOpenTask {
        _temp,
        store,
        task_path,
    } = staged_open_task(None, body);
    let before = std::fs::read_to_string(&task_path).unwrap();

    apply_tag_patch(&store, None);

    assert_eq!(std::fs::read_to_string(task_path).unwrap(), before);
}

#[test]
fn generic_update_sets_tags_on_bom_frontmatter() {
    let before = formatted_tag_note(true, "\n", None);
    let StagedOpenTask {
        _temp,
        store,
        task_path,
    } = staged_open_task_from_note(&before);

    apply_tag_patch(&store, Some(sqlite_tags()));

    assert_eq!(
        std::fs::read_to_string(task_path).unwrap(),
        formatted_tag_note(true, "\n", Some("[sqlite]"))
    );
}

#[test]
fn generic_update_clears_tags_on_bom_frontmatter() {
    let before = formatted_tag_note(true, "\n", Some("[godot]"));
    let StagedOpenTask {
        _temp,
        store,
        task_path,
    } = staged_open_task_from_note(&before);

    apply_tag_patch(&store, None);

    assert_eq!(
        std::fs::read_to_string(task_path).unwrap(),
        formatted_tag_note(true, "\n", None)
    );
}

#[test]
fn generic_update_sets_tags_on_crlf_frontmatter() {
    let before = formatted_tag_note(false, "\r\n", None);
    let StagedOpenTask {
        _temp,
        store,
        task_path,
    } = staged_open_task_from_note(&before);

    apply_tag_patch(&store, Some(sqlite_tags()));

    assert_eq!(
        std::fs::read_to_string(task_path).unwrap(),
        formatted_tag_note(false, "\r\n", Some("[sqlite]"))
    );
}

#[test]
fn generic_update_clears_tags_on_crlf_frontmatter() {
    let before = formatted_tag_note(false, "\r\n", Some("[godot]"));
    let StagedOpenTask {
        _temp,
        store,
        task_path,
    } = staged_open_task_from_note(&before);

    apply_tag_patch(&store, None);

    assert_eq!(
        std::fs::read_to_string(task_path).unwrap(),
        formatted_tag_note(false, "\r\n", None)
    );
}

#[test]
fn generic_delete_moves_note_to_vault_trash_and_preserves_project_page() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::create_dir(notes_dir.join(".trash")).unwrap();
    std::fs::write(project_dir.join("foo.md"), "- [ ] [[FOO-0001]]\n").unwrap();
    let task_path = project_dir.join("FOO-0001.md");
    write_note(
        &task_path,
        "stale task",
        "2026-07-01",
        None,
        None,
        None,
        "remove me",
    );
    let store = store_with_index_identity(&notes_dir.join("foo"));
    let mut project = foo_project(&store);
    project.obsidian_vault =
        Some(pwf_models::project::ObsidianVault::try_new(path_str(&notes_dir)).unwrap());
    let id = TaskId::try_new("FOO-0001").unwrap();

    commit_for_task(
        &store,
        &project,
        &id,
        vec![TaskWrite::DeleteNote {
            id: id.clone(),
            deletion: pwf_wire::confirmation::TaskDeletion::MoveToTrash {
                obsidian_vault: notes_dir.clone(),
            },
        }],
    );

    assert!(!task_path.exists());
    assert_eq!(
        std::fs::read_to_string(notes_dir.join(".trash/FOO-0001.md")).unwrap(),
        "---\nid: FOO-0001\nstatus: active\ntitle: stale task\nproject: foo\ncreated_at: 2026-07-01T00:00:00Z\n---\n\nremove me\n"
    );
    assert_eq!(
        std::fs::read_to_string(project_dir.join("foo.md")).unwrap(),
        "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0001]]\n"
    );
}

#[test]
fn repeated_deletion_preserves_each_note_in_numbered_trash_files() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    let store = store_with_index_identity(&project_dir);
    std::fs::create_dir_all(notes_dir.join(".trash")).unwrap();
    let mut project = foo_project(&store);
    project.obsidian_vault =
        Some(pwf_models::project::ObsidianVault::try_new(path_str(&notes_dir)).unwrap());
    let id = TaskId::try_new("FOO-0001").unwrap();
    let task_path = project_dir.join("FOO-0001.md");
    let mut saved = Vec::new();

    for name in ["FOO-0001.md", "FOO-0001 (1).md", "FOO-0001 (2).md"] {
        generic_add(&store, new_task(name, "reused task ID")).unwrap();
        let contents = std::fs::read(&task_path).unwrap();
        commit_for_task(
            &store,
            &project,
            &id,
            vec![TaskWrite::DeleteNote {
                id: id.clone(),
                deletion: pwf_wire::confirmation::TaskDeletion::MoveToTrash {
                    obsidian_vault: notes_dir.clone(),
                },
            }],
        );
        saved.push((notes_dir.join(".trash").join(name), contents));
        assert!(!task_path.exists());
        assert!(TaskVault::list_tasks(&store, &project).unwrap().is_empty());
        assert!(
            !std::fs::read_to_string(project_dir.join("foo.md"))
                .unwrap()
                .contains("FOO-0001")
        );
        for (path, expected) in &saved {
            assert_eq!(&std::fs::read(path).unwrap(), expected);
        }
    }
}

#[test]
fn note_backed_record_revision_hashes_the_complete_persisted_note() {
    let staged = staged_open_task(None, "body with exact bytes\r\n");
    let project = foo_project(&staged.store);
    let id = TaskId::try_new("FOO-0001").unwrap();

    let record = TaskVault::get_task_record(&staged.store, &project, &id)
        .unwrap()
        .unwrap();

    assert_eq!(
        record.revision,
        content_revision(&std::fs::read(&staged.task_path).unwrap())
    );
}

#[test]
fn stale_note_patch_preserves_the_external_edit_and_index() {
    let staged = staged_open_task(None, "original body");
    let project = foo_project(&staged.store);
    let id = TaskId::try_new("FOO-0001").unwrap();
    let record = TaskVault::get_task_record(&staged.store, &project, &id)
        .unwrap()
        .unwrap();
    let index_path = staged.task_path.parent().unwrap().join("foo.md");
    let index_before = std::fs::read(&index_path).unwrap();
    let external = format!("{}\nexternal edit\n", record.source);
    std::fs::write(&staged.task_path, &external).unwrap();
    let writes = TaskWriteSet::try_new(
        vec![ExpectedTaskRevision {
            id: id.clone(),
            revision: record.revision,
        }],
        vec![TaskWrite::Patch {
            id,
            patch: TaskPatch {
                title: SetField::Set(TaskTitle::try_new("local edit").unwrap()),
                ..TaskPatch::default()
            },
        }],
    )
    .unwrap();

    let error = TaskVault::commit_task_writes(&staged.store, &project, writes).unwrap_err();

    assert!(matches!(error, TaskMutationError::StaleTask { .. }));
    assert_eq!(std::fs::read_to_string(staged.task_path).unwrap(), external);
    assert_eq!(std::fs::read(index_path).unwrap(), index_before);
}

#[test]
fn note_only_patch_does_not_require_the_project_index() {
    let staged = staged_open_task(None, "original body");
    let project = foo_project(&staged.store);
    let id = TaskId::try_new("FOO-0001").unwrap();
    let record = TaskVault::get_task_record(&staged.store, &project, &id)
        .unwrap()
        .unwrap();
    let index_path = staged.task_path.parent().unwrap().join("foo.md");
    std::fs::remove_file(&index_path).unwrap();
    let writes = TaskWriteSet::try_new(
        vec![ExpectedTaskRevision {
            id: id.clone(),
            revision: record.revision,
        }],
        vec![TaskWrite::Patch {
            id,
            patch: TaskPatch {
                title: SetField::Set(TaskTitle::try_new("updated without index").unwrap()),
                ..TaskPatch::default()
            },
        }],
    )
    .unwrap();

    TaskVault::commit_task_writes(&staged.store, &project, writes).unwrap();

    assert!(
        std::fs::read_to_string(staged.task_path)
            .unwrap()
            .contains("title: updated without index")
    );
    assert!(!index_path.exists());
}

#[test]
fn stale_delete_preserves_the_external_edit_index_and_trash_state() {
    let temp = tempfile::tempdir().unwrap();
    let vault = temp.path().join("vault");
    let project_dir = vault.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::create_dir(vault.join(".obsidian")).unwrap();
    let index_path = project_dir.join("foo.md");
    std::fs::write(&index_path, "- [ ] [[FOO-0001]]\n").unwrap();
    ensure_test_index_identity(&index_path, "foo", "foo");
    let task_path = project_dir.join("FOO-0001.md");
    write_note(
        &task_path,
        "remove me",
        "2026-07-01",
        None,
        None,
        None,
        "original",
    );
    let store = store_for_tasks(&project_dir);
    let project = foo_project(&store);
    let id = TaskId::try_new("FOO-0001").unwrap();
    let record = TaskVault::get_task_record(&store, &project, &id)
        .unwrap()
        .unwrap();
    let index_before = std::fs::read(&index_path).unwrap();
    let external = format!("{}\nexternal edit\n", record.source);
    std::fs::write(&task_path, &external).unwrap();
    let writes = TaskWriteSet::try_new(
        vec![ExpectedTaskRevision {
            id: id.clone(),
            revision: record.revision,
        }],
        vec![TaskWrite::DeleteNote {
            id,
            deletion: pwf_wire::confirmation::TaskDeletion::MoveToTrash {
                obsidian_vault: vault.clone(),
            },
        }],
    )
    .unwrap();

    let error = TaskVault::commit_task_writes(&store, &project, writes).unwrap_err();

    assert!(matches!(error, TaskMutationError::StaleTask { .. }));
    assert_eq!(std::fs::read_to_string(task_path).unwrap(), external);
    assert_eq!(std::fs::read(index_path).unwrap(), index_before);
    assert!(!vault.join(".trash").exists());
}

#[test]
fn get_returns_open_note_locator_from_active_index() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("foo.md"), "- [ ] [[FOO-0001]]\n").unwrap();
    write_note(
        &project_dir.join("FOO-0001.md"),
        "active task",
        "2026-07-01",
        None,
        None,
        None,
        "body",
    );
    let store = store_with_index_identity(&notes_dir.join("foo"));

    let record = get_record(&store, "FOO-0001").unwrap();

    assert_eq!(record.locator.as_path(), project_dir.join("FOO-0001.md"));
}

#[test]
fn get_returns_open_note_source_with_created_at_key() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("foo.md"), "- [ ] [[FOO-0001]]\n").unwrap();
    write_note(
        &project_dir.join("FOO-0001.md"),
        "active task",
        "2026-07-01",
        None,
        None,
        None,
        "## Goals\n- body",
    );
    let store = store_with_index_identity(&notes_dir.join("foo"));

    let record = get_record(&store, "FOO-0001").unwrap();

    assert_eq!(
        record.source,
        "---\nid: FOO-0001\nstatus: active\ntitle: active task\nproject: foo\ncreated_at: 2026-07-01T00:00:00Z\n---\n\n## Goals\n- body\n"
    );
}

#[test]
fn get_finds_closed_note_still_in_project_dir() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("foo.md"),
        "- [x] [[FOO-0003]] ✅ 2026-07-07\n",
    )
    .unwrap();
    write_status_note(
        &project_dir.join("FOO-0003.md"),
        "done task",
        "done",
        Some("2026-07-07"),
        None,
    );
    let store = store_with_index_identity(&notes_dir.join("foo"));

    let record = get_record(&store, "FOO-0003").unwrap();

    assert_eq!(
        record.source,
        "---\nid: FOO-0003\nstatus: done\ntitle: done task\nproject: foo\ncreated_at: 2026-07-01T00:00:00Z\ncompleted_at: 2026-07-07T00:00:00Z\n---\n\nbody\n"
    );
}

fn store_with_index_identity(tasks_path: &Path) -> ObsidianStore {
    std::fs::create_dir_all(tasks_path).unwrap();
    let page = tasks_path.join("foo.md");
    if !page.exists() {
        std::fs::write(&page, "---\nid: foo\ntitle: foo\n---\n\n").unwrap();
    }
    ensure_test_index_identity(&page, "foo", "foo");
    store_for_tasks(tasks_path)
}

fn store_for_tasks(tasks_path: &Path) -> ObsidianStore {
    ObsidianStore::new(HomeDirectory::new(tasks_path.to_path_buf()))
}

struct StagedOpenTask {
    _temp: tempfile::TempDir,
    store: ObsidianStore,
    task_path: std::path::PathBuf,
}

fn staged_open_task_with_tags(tags: &str) -> StagedOpenTask {
    staged_open_task(Some(tags), "body")
}

fn staged_open_task(tags: Option<&str>, body: &str) -> StagedOpenTask {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("foo.md"), "- [ ] [[FOO-0001]]\n").unwrap();
    let task_path = project_dir.join("FOO-0001.md");
    write_note(&task_path, "tagged", "2026-07-01", None, None, tags, body);
    let store = store_with_index_identity(&notes_dir.join("foo"));
    StagedOpenTask {
        _temp: temp,
        store,
        task_path,
    }
}

fn staged_open_task_from_note(note: &str) -> StagedOpenTask {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("foo.md"), "- [ ] [[FOO-0001]]\n").unwrap();
    let task_path = project_dir.join("FOO-0001.md");
    std::fs::write(&task_path, note).unwrap();
    let store = store_with_index_identity(&notes_dir.join("foo"));
    StagedOpenTask {
        _temp: temp,
        store,
        task_path,
    }
}

fn formatted_tag_note(bom: bool, newline: &str, tags: Option<&str>) -> String {
    let bom = if bom { "\u{feff}" } else { "" };
    let tags = tags.map_or_else(String::new, |tags| format!("tags: {tags}{newline}"));
    format!(
        "{bom}---{newline}id: FOO-0001{newline}status: active{newline}title: tagged{newline}project: foo{newline}created_at: 2026-07-01T00:00:00Z{newline}{tags}---{newline}{newline}tags: body-only value{newline}keep this body byte-identical{newline}"
    )
}

fn ensure_test_index_identity(path: &Path, id: &str, title: &str) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    if content.starts_with("---") {
        return;
    }
    std::fs::write(
        path,
        format!("---\nid: {id}\ntitle: {title}\n---\n\n{content}"),
    )
    .unwrap();
}

fn write_status_note(
    path: &Path,
    title: &str,
    status: &str,
    completed_date: Option<&str>,
    commits: Option<&str>,
) {
    let id = path.file_stem().and_then(|stem| stem.to_str()).unwrap();
    let mut note = format!(
        "---\nid: {id}\nstatus: {status}\ntitle: {title}\nproject: foo\ncreated_at: 2026-07-01T00:00:00Z\n"
    );
    if let Some(completed_date) = completed_date {
        let _ = writeln!(note, "completed_at: {completed_date}T00:00:00Z");
    }
    if let Some(commits) = commits {
        let _ = writeln!(note, "commits: \"{commits}\"");
    }
    note.push_str("---\n\nbody\n");
    std::fs::write(path, note).unwrap();
}

#[test]
fn task_record_roundtrips_file_model_note() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("foo.md"), "- [ ] [[FOO-0001]]\n").unwrap();
    let note_path = project_dir.join("FOO-0001.md");
    let source = concat!(
        "---\n",
        "id: FOO-0001\n",
        "status: active\n",
        "title: ship the adapter\n",
        "project: foo\n",
        "created_at: 2026-07-01T00:00:00Z\n",
        "blocked_by:\n",
        "  - \"[[AUX-0001]]\"\n",
        "effort: medium\n",
        "priority: highest\n",
        "tags: [sqlite, godot]\n",
        "---\n",
        "\n",
        "ship the adapter body\n",
    );
    std::fs::write(&note_path, source).unwrap();
    let store = store_with_index_identity(&notes_dir.join("foo"));

    let project = foo_project(&store);
    let id = TaskId::try_new("FOO-0001").unwrap();
    let record = TaskVault::get_task_record(&store, &project, &id)
        .unwrap()
        .unwrap();

    assert_eq!(record.id, id);
    assert_eq!(record.title, "ship the adapter");
    assert_eq!(record.status, TaskStatus::Active);
    assert_eq!(
        record.created_at,
        Some(task_timestamp("2026-07-01T00:00:00Z"))
    );
    assert_eq!(record.completed_at, None);
    assert_eq!(record.commits, None);
    assert_eq!(
        record.blocked_by,
        StoredBlockedBy::Valid(
            BlockedBy::try_new(vec![TaskId::try_new("AUX-0001").unwrap()]).unwrap()
        )
    );
    assert_eq!(record.effort.as_deref(), Some("medium"));
    assert_eq!(record.priority.as_deref(), Some("highest"));
    assert_eq!(
        record.tags.as_ref().map(AsRef::as_ref),
        Some("[sqlite, godot]")
    );
    assert_eq!(record.body, "\nship the adapter body\n");
    assert_eq!(record.locator.as_path(), note_path);
    assert_eq!(record.source, source);
}

#[test]
fn task_record_preserves_malformed_blocked_by_without_failing_the_read() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("foo.md"), "- [ ] [[FOO-0001]]\n").unwrap();
    let note_path = project_dir.join("FOO-0001.md");
    write_note(
        &note_path,
        "malformed dependency",
        "2026-07-01",
        Some("\"[[AUX-0001]]\""),
        None,
        None,
        "body",
    );
    let store = store_with_index_identity(&project_dir);

    let record = get_record(&store, "FOO-0001").unwrap();

    assert!(matches!(
        record.blocked_by,
        StoredBlockedBy::Malformed { ref raw, ref reason }
            if raw == "\"[[AUX-0001]]\"" && reason.contains("sequence")
    ));
}

#[test]
fn task_record_rejects_an_invalid_created_at_timestamp() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("foo.md"), "- [ ] [[FOO-0001]]\n").unwrap();
    let note_path = project_dir.join("FOO-0001.md");
    write_note(
        &note_path,
        "invalid date",
        "2026-02-30",
        None,
        None,
        None,
        "body",
    );
    let store = store_with_index_identity(&project_dir);
    let project = foo_project(&store);

    let error = TaskVault::get_task_record(&store, &project, &TaskId::try_new("FOO-0001").unwrap())
        .unwrap_err();

    assert_matches!(
        error,
        ObsidianStoreError::InvalidTaskTimestamp {
            path,
            property: "created_at",
            ref value,
            ..
        } if path == note_path && value == "2026-02-30T00:00:00Z"
    );
}

#[test]
fn task_record_rejects_an_invalid_status() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("foo.md"), "- [ ] [[FOO-0001]]\n").unwrap();
    let note_path = project_dir.join("FOO-0001.md");
    std::fs::write(
        &note_path,
        "---\nid: FOO-0001\nstatus: paused\ntitle: invalid status\n---\n\nbody\n",
    )
    .unwrap();
    let store = store_with_index_identity(&project_dir);
    let project = foo_project(&store);

    let error = TaskVault::get_task_record(&store, &project, &TaskId::try_new("FOO-0001").unwrap())
        .unwrap_err();

    assert_matches!(
        error,
        ObsidianStoreError::InvalidTaskStatus {
            path,
            ref value,
            ..
        } if path == note_path && value == "paused"
    );
}

#[test]
fn insert_allocates_next_id_without_index_write() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    let index_path = project_dir.join("foo.md");
    let index_before = "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0007]]\n";
    std::fs::write(&index_path, index_before).unwrap();
    write_note(
        &project_dir.join("FOO-0007.md"),
        "existing",
        "2026-07-01",
        None,
        None,
        None,
        "already here",
    );
    let store = store_for_tasks(&notes_dir.join("foo"));

    let project = foo_project(&store);
    let record = insert_next(
        &store,
        &project,
        NewTask {
            body: "wire up the new thing".to_string().into(),
            title: TaskTitle::try_new("wire up the new thing").unwrap(),
            created_at: task_timestamp("2026-07-15T12:34:56Z"),
            blocked_by: None,
            effort: None,
            priority: None,
            tags: None,
        },
    )
    .unwrap();

    assert_eq!(record.id, TaskId::try_new("FOO-0008").unwrap());
    assert_eq!(record.status, TaskStatus::Active);
    assert!(record.source.contains("id: FOO-0008"));
    assert_eq!(record.locator.as_path(), project_dir.join("FOO-0008.md"));
    assert!(project_dir.join("FOO-0008.md").exists());
    assert_eq!(std::fs::read_to_string(&index_path).unwrap(), index_before);
}

#[test]
fn exact_insert_rejects_an_id_occupied_after_allocation_without_replacing_it() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    let store = store_with_index_identity(&project_dir);
    let project = foo_project(&store);
    let id = TaskVault::next_task_id(&store, &project).unwrap();
    let path = project_dir.join("FOO-0001.md");
    std::fs::create_dir_all(&project_dir).unwrap();
    write_note(
        &path,
        "concurrent task",
        "2026-07-15",
        None,
        None,
        None,
        "keep me",
    );
    let before = std::fs::read_to_string(&path).unwrap();

    let error = TaskVault::insert_task(
        &store,
        &project,
        &id,
        new_task("replacement", "replacement"),
    )
    .unwrap_err();

    assert_matches!(
        error,
        ObsidianStoreError::TaskIdOccupied { ref id, path: ref error_path }
            if id.as_ref() == "FOO-0001" && error_path == &path
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), before);
}

#[test]
fn list_tasks_returns_note_history_and_ignores_missing_notes() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    let index_path = project_dir.join("foo.md");
    std::fs::write(
        &index_path,
        concat!(
            "---\nid: foo\ntitle: foo\n---\n\n",
            "- [ ] [[FOO-0001|linked active]]\n",
            "## Human\n",
            "- [x] [[FOO-0002|linked done]] ✅ 2026-07-02\n",
            "- [x] [[FOO-0006|missing done]] ✅ 2026-07-06\n",
        ),
    )
    .unwrap();
    for (id, status, completed) in [
        ("FOO-0001", "active", None),
        ("FOO-0002", "done", Some("2026-07-02")),
        ("FOO-0003", "cancelled", Some("2026-07-03")),
        ("FOO-0004", "done", Some("2026-07-04")),
        ("FOO-0005", "active", None),
    ] {
        write_status_note(
            &project_dir.join(format!("{id}.md")),
            &format!("title {id}"),
            status,
            completed,
            None,
        );
    }
    let store = store_for_tasks(&notes_dir.join("foo"));
    let project = foo_project(&store);

    let records = TaskVault::list_tasks(&store, &project).unwrap();
    assert_eq!(
        TaskVault::list_task_summaries(&store, &project).unwrap(),
        records
            .iter()
            .cloned()
            .map(pwf_application::ports::task_vault::TaskSummaryRecord::from)
            .collect::<Vec<_>>()
    );
    let mut ids: Vec<String> = records.iter().map(|record| record.id.to_string()).collect();
    ids.sort();

    assert_eq!(
        ids,
        ["FOO-0001", "FOO-0002", "FOO-0003", "FOO-0004", "FOO-0005",]
    );
    let record = |id: &str| {
        records
            .iter()
            .find(|record| record.id.as_ref() == id)
            .unwrap()
    };
    assert_eq!(record("FOO-0001").status, TaskStatus::Active);
    assert_eq!(record("FOO-0002").status, TaskStatus::Done);
    assert_eq!(record("FOO-0003").status, TaskStatus::Cancelled);
    assert_eq!(record("FOO-0004").status, TaskStatus::Done);
    assert_eq!(record("FOO-0005").status, TaskStatus::Active);
    assert!(get_record(&store, "FOO-0006").is_none());
}

#[test]
fn list_tasks_returns_note_history_when_index_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    write_status_note(
        &project_dir.join("FOO-0001.md"),
        "evicted done task",
        "done",
        Some("2026-07-01"),
        None,
    );
    let store = store_for_tasks(&notes_dir.join("foo"));
    let project = foo_project(&store);

    let records = TaskVault::list_tasks(&store, &project).unwrap();
    assert_eq!(
        TaskVault::list_task_summaries(&store, &project).unwrap(),
        records
            .iter()
            .cloned()
            .map(pwf_application::ports::task_vault::TaskSummaryRecord::from)
            .collect::<Vec<_>>()
    );

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TaskStatus::Done);
}

#[test]
fn missing_registered_trash_preserves_the_note_and_index() {
    for occupied_by_file in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let store = store_with_index_identity(&root.path().join("tasks"));
        let mut project = foo_project(&store);
        let vault = root.path().join("separate-vault");
        std::fs::create_dir(&vault).unwrap();
        project.obsidian_vault =
            Some(pwf_models::project::ObsidianVault::try_new(path_str(&vault)).unwrap());
        let record = generic_add(&store, new_task("keep me", "original bytes")).unwrap();
        let note = record.locator.as_path();
        let index = store.home.as_path().join("foo.md");
        let note_before = std::fs::read(note).unwrap();
        let index_before = std::fs::read(&index).unwrap();
        if occupied_by_file {
            std::fs::write(vault.join(".trash"), "not a folder").unwrap();
        }
        assert!(TaskVault::task_deletion(&store, &project).is_err());
        let writes = TaskWriteSet::try_new(
            vec![ExpectedTaskRevision {
                id: record.id.clone(),
                revision: record.revision,
            }],
            vec![TaskWrite::DeleteNote {
                id: record.id,
                deletion: pwf_wire::confirmation::TaskDeletion::MoveToTrash {
                    obsidian_vault: vault.clone(),
                },
            }],
        )
        .unwrap();
        assert!(TaskVault::commit_task_writes(&store, &project, writes).is_err());
        assert_eq!(std::fs::read(note).unwrap(), note_before);
        assert_eq!(std::fs::read(index).unwrap(), index_before);
        assert!(!vault.join(".trash").is_dir());
    }
}

#[test]
fn trash_removed_after_preflight_preserves_the_note_and_index() {
    let root = tempfile::tempdir().unwrap();
    let store = store_with_index_identity(&root.path().join("tasks"));
    let mut project = foo_project(&store);
    let vault = root.path().join("separate-vault");
    std::fs::create_dir_all(vault.join(".trash")).unwrap();
    project.obsidian_vault =
        Some(pwf_models::project::ObsidianVault::try_new(path_str(&vault)).unwrap());
    let record = generic_add(&store, new_task("keep me", "original bytes")).unwrap();
    let index = store.home.as_path().join("foo.md");
    let index_before = std::fs::read(&index).unwrap();
    let deletion = TaskVault::task_deletion(&store, &project).unwrap();
    std::fs::remove_dir(vault.join(".trash")).unwrap();
    let writes = TaskWriteSet::try_new(
        vec![ExpectedTaskRevision {
            id: record.id.clone(),
            revision: record.revision.clone(),
        }],
        vec![TaskWrite::DeleteNote {
            id: record.id.clone(),
            deletion,
        }],
    )
    .unwrap();
    assert!(TaskVault::commit_task_writes(&store, &project, writes).is_err());
    assert_eq!(
        std::fs::read_to_string(record.locator.as_path()).unwrap(),
        record.source
    );
    assert_eq!(std::fs::read(index).unwrap(), index_before);
    assert!(!vault.join(".trash").exists());
}

#[test]
fn unregistered_project_hard_deletes_even_under_an_obsidian_vault() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".obsidian")).unwrap();
    std::fs::create_dir(root.path().join(".trash")).unwrap();
    let store = store_with_index_identity(&root.path().join("tasks"));
    let project = foo_project(&store);
    let record = generic_add(&store, new_task("remove me", "original bytes")).unwrap();
    let deletion = TaskVault::task_deletion(&store, &project).unwrap();
    assert_eq!(deletion, pwf_wire::confirmation::TaskDeletion::HardDelete);
    commit_for_task(
        &store,
        &project,
        &record.id,
        vec![TaskWrite::DeleteNote {
            id: record.id.clone(),
            deletion,
        }],
    );
    assert!(!record.locator.as_path().exists());
    assert!(TaskVault::list_tasks(&store, &project).unwrap().is_empty());
    assert_eq!(
        std::fs::read_dir(root.path().join(".trash"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn dependency_reads_ignore_bodies_and_unrelated_semantic_metadata() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = temporary_directory.path();
    std::fs::write(path.join("FOO-0001.md"), b"---\nid: FOO-0001\nstatus: invalid\ncreated_at: invalid\nblocked_by: [\"[[FOO-0002]]\"]\n---\n\xff").unwrap();
    std::fs::write(path.join("FOO-0002.md"), b"---\nid: FOO-0002\n---\n\xff").unwrap();
    let store = store_for_tasks(path);
    let dependencies = TaskVault::get_task_dependencies(
        &store,
        &foo_project(&store),
        &"FOO-0001".parse().unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        dependencies
            .blocked_by
            .valid()
            .unwrap()
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>(),
        ["FOO-0002"]
    );
}

#[test]
fn single_task_read_does_not_load_other_task_bodies() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = temporary_directory.path();
    std::fs::write(
        path.join("selected.md"),
        "---\nid: FOO-0001\ntitle: selected\n---\nselected body",
    )
    .unwrap();
    std::fs::write(path.join("other.md"), b"---\nid: FOO-0002\n---\n\xff").unwrap();
    let store = store_for_tasks(path);
    let record = get_record(&store, "FOO-0001").unwrap();
    assert_eq!(record.title, "selected");
    assert!(record.body.contains("selected body"));
}

#[test]
fn task_crud_ignores_authored_and_generated_page_contents() {
    for page in [
        &b"---\nid: FOO-9999\nstatus: broken\n---\n## Later\n- [x] [[FOO-0001]]\n- [ ] [[FOO-0002]]\n- [ ] [[FOO-0002]]\n"[..],
        &b"---\ninvalid: [\n---\n"[..],
        &b"\xff\xfe"[..],
    ] {
        let directory = tempfile::tempdir().unwrap();
        let store = store_for_tasks(directory.path());
        let project = foo_project(&store);
        for name in ["foo.md", "pwf-index.md"] {
            std::fs::write(directory.path().join(name), page).unwrap();
        }
        let record = insert_next(&store, &project, new_task("body", "Task")).unwrap();
        assert_eq!(record.id.as_ref(), "FOO-0001");
        assert_eq!(get_record(&store, "FOO-0001").unwrap(), record);
        assert_eq!(TaskVault::list_tasks(&store, &project).unwrap(), std::slice::from_ref(&record));
        assert_eq!(TaskVault::list_task_summaries(&store, &project).unwrap().len(), 1);
        assert!(TaskVault::get_task_dependencies(&store, &project, &record.id).unwrap().is_some());
        assert!(get_record(&store, "FOO-0002").is_none());
        assert!(get_record(&store, "FOO-9999").is_none());
        for status in [TaskStatus::Done, TaskStatus::Cancelled, TaskStatus::Active] {
            commit_for_task(&store, &project, &record.id, vec![TaskWrite::Patch {
                id: record.id.clone(),
                patch: TaskPatch { status: SetField::Set(status), ..TaskPatch::default() },
            }]);
            assert_eq!(get_record(&store, "FOO-0001").unwrap().status, status);
        }
        commit_for_task(&store, &project, &record.id, vec![TaskWrite::DeleteNote {
            id: record.id.clone(),
            deletion: pwf_wire::confirmation::TaskDeletion::HardDelete,
        }]);
        assert!(TaskVault::list_tasks(&store, &project).unwrap().is_empty());
        assert!(get_record(&store, "FOO-0001").is_none());
        for name in ["foo.md", "pwf-index.md"] {
            assert_eq!(std::fs::read(directory.path().join(name)).unwrap(), page);
        }
    }
}

#[test]
fn task_crud_does_not_create_or_read_project_pages() {
    for unreadable_pages in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let store = store_for_tasks(directory.path());
        let project = foo_project(&store);
        if unreadable_pages {
            for name in ["foo.md", "pwf-index.md"] {
                std::fs::create_dir(directory.path().join(name)).unwrap();
            }
        }
        let record = insert_next(&store, &project, new_task("body", "Task")).unwrap();
        commit_for_task(
            &store,
            &project,
            &record.id,
            vec![TaskWrite::Patch {
                id: record.id.clone(),
                patch: TaskPatch {
                    title: SetField::Set(TaskTitle::try_new("Edited").unwrap()),
                    ..TaskPatch::default()
                },
            }],
        );
        assert_eq!(get_record(&store, "FOO-0001").unwrap().title, "edited");
        commit_for_task(
            &store,
            &project,
            &record.id,
            vec![TaskWrite::DeleteNote {
                id: record.id.clone(),
                deletion: pwf_wire::confirmation::TaskDeletion::HardDelete,
            }],
        );
        for name in ["foo.md", "pwf-index.md"] {
            let path = directory.path().join(name);
            assert_eq!(path.exists(), unreadable_pages);
            assert_eq!(path.is_dir(), unreadable_pages);
        }
    }
}

#[test]
fn exact_insert_rejects_an_existing_frontmatter_id_at_a_descriptive_path() {
    let directory = tempfile::tempdir().unwrap();
    let store = store_for_tasks(directory.path());
    let path = directory.path().join("descriptive.md");
    let source = "---\nid: FOO-0001\ntitle: Existing\n---\nbody\n";
    std::fs::write(&path, source).unwrap();
    let error = TaskVault::insert_task(
        &store,
        &foo_project(&store),
        &"FOO-0001".parse().unwrap(),
        new_task("new", "New"),
    )
    .unwrap_err();
    assert_matches!(error, ObsidianStoreError::TaskIdOccupied { path: occupied, .. } if occupied == path);
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    assert!(!directory.path().join("FOO-0001.md").exists());
}

#[test]
fn guarded_batch_rejects_a_stale_note_before_changing_any_note() {
    let directory = tempfile::tempdir().unwrap();
    let store = store_for_tasks(directory.path());
    let project = foo_project(&store);
    let first = insert_next(&store, &project, new_task("first", "First")).unwrap();
    let second = insert_next(&store, &project, new_task("second", "Second")).unwrap();
    let writes = TaskWriteSet::try_new(
        [&first, &second]
            .map(|record| ExpectedTaskRevision {
                id: record.id.clone(),
                revision: record.revision.clone(),
            })
            .to_vec(),
        [&first, &second]
            .map(|record| TaskWrite::DeleteNote {
                id: record.id.clone(),
                deletion: pwf_wire::confirmation::TaskDeletion::HardDelete,
            })
            .to_vec(),
    )
    .unwrap();
    let external = format!("{}external edit\n", second.source);
    std::fs::write(second.locator.as_path(), &external).unwrap();
    let error = TaskVault::commit_task_writes(&store, &project, writes).unwrap_err();
    assert_matches!(error, TaskMutationError::StaleTask { id, .. } if id == second.id);
    assert_eq!(get_record(&store, "FOO-0001").unwrap().source, first.source);
    assert_eq!(get_record(&store, "FOO-0002").unwrap().source, external);
}
