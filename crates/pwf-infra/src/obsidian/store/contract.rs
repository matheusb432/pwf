use std::{assert_matches, fmt::Write as _, num::NonZeroUsize, path::Path};

use pwf_application::ports::task_record::{
    ExpectedTaskRevision, IndexEntry, IndexEntryState, IndexEntryStore, IndexPlacement,
    IndexSectionStore, Materialization, NewTask, NullablePatch, StoredBlockedBy, TaskMutationError,
    TaskMutationStore, TaskPatch, TaskRecord, TaskStore, TaskWrite, TaskWriteSet,
};
use pwf_models::{
    project::{
        HomeDirectory, Project, ProjectId, ProjectName, ProjectSource, ProjectSourceKind,
        ProjectSourceValue, ProjectTasks, ProjectTasksKind, ProjectTasksPath,
    },
    task::{
        BlockedBy, EffortTier, PriorityTier, Tag, TaskId, TaskSection, TaskStatus, TaskTags,
        TaskTimestamp, TaskTitle,
    },
};
use pwf_wire::task::{TaskIndexPath, TaskNotePath};

use super::{ObsidianStore, ObsidianStoreError, fs::path_str};
use crate::file_transaction::content_revision;

const S: &str = "\n\n";

fn task_timestamp(raw: &str) -> TaskTimestamp {
    raw.parse().unwrap()
}

fn task_section(raw: &str) -> TaskSection {
    raw.parse().unwrap()
}

fn project(id: &str, title: &str, tasks_path: &Path) -> Project {
    Project {
        id: ProjectId::try_new(id.to_ascii_uppercase()).unwrap(),
        title: ProjectName::try_new(title).unwrap(),
        source: ProjectSource::new(
            ProjectSourceKind::Directory,
            ProjectSourceValue::try_new(format!("/projects/{title}")).unwrap(),
        ),
        tasks: ProjectTasks::new(
            ProjectTasksKind::Directory,
            ProjectTasksPath::try_new(path_str(tasks_path)).unwrap(),
        ),
        created_at: "2026-07-25T00:00:00.000Z".parse().unwrap(),
        is_paused: false,
    }
}

fn foo_project(store: &ObsidianStore) -> Project {
    project("FOO", "foo", store.home.as_path())
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

    let records = TaskStore::list(&store, &project).unwrap();

    assert!(records.is_empty());
    assert!(!tasks_path.exists());
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

    let records = TaskStore::list(&store, &project).unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, TaskId::try_new("FOO-0001").unwrap());
}

#[test]
fn explicit_index_path_uses_the_project_title_inside_tasks_path() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let tasks_path = temporary_directory.path().join("records");
    std::fs::create_dir_all(&tasks_path).unwrap();
    let index_path = tasks_path.join("sample-project.md");
    std::fs::write(
        &index_path,
        "---\nid: smp\ntitle: sample-project\n---\n\n- [ ] [[SMP-0001]]\n",
    )
    .unwrap();
    let store = ObsidianStore::new(HomeDirectory::new(tasks_path.clone()));
    let project_name = project("SMP", "sample-project", &tasks_path);

    let records = TaskStore::list(&store, &project_name).unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].placement.as_ref().unwrap().index_path.as_path(),
        index_path
    );
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
    }
    let store = ObsidianStore::new(HomeDirectory::new(temporary_directory.path().to_path_buf()));

    for (title, expected_id) in [("alpha", "AAA-0001"), ("beta", "BBB-0001")] {
        let tasks_path = if title == "alpha" {
            &first_tasks
        } else {
            &second_tasks
        };
        let project_name = project(&expected_id[..3], title, tasks_path);
        let records = TaskStore::list(&store, &project_name).unwrap();
        assert_eq!(records[0].id.as_ref(), expected_id);
    }
}

#[test]
fn explicit_index_validation_uses_the_supplied_identity() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let tasks_path = temporary_directory.path().join("tasks");
    std::fs::create_dir_all(&tasks_path).unwrap();
    std::fs::write(
        tasks_path.join("foo.md"),
        "---\nid: old\ntitle: foo\n---\n\n",
    )
    .unwrap();
    let store = ObsidianStore::new(HomeDirectory::new(tasks_path.clone()));
    let project_name = project("NEW", "foo", &tasks_path);

    let error = TaskStore::list(&store, &project_name).unwrap_err();

    assert_matches!(
        error,
        ObsidianStoreError::ProjectIndexIdentityMismatch {
            ref expected_id,
            ..
        } if expected_id.as_ref() == "NEW"
    );
}

#[test]
fn generic_list_rejects_project_index_without_identity_frontmatter() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("foo.md"), "- [ ] [[FOO-0001]]\n").unwrap();
    write_note(
        &project_dir.join("FOO-0001.md"),
        "task",
        "2026-07-12",
        None,
        None,
        None,
        "body",
    );
    let store = store_for_tasks(&notes_dir.join("foo"));

    let project = foo_project(&store);
    let error = TaskStore::list(&store, &project).unwrap_err();

    assert_matches!(
        error,
        ObsidianStoreError::MissingFrontmatter { property, .. }
            if property == "id/title"
    );
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

    let error = TaskStore::list(&store, &project).unwrap_err();

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
    TaskStore::get(store, &project, &TaskId::try_new(id).unwrap()).unwrap()
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

    let error =
        TaskStore::get(&store, &project, &TaskId::try_new("FOO-0001").unwrap()).unwrap_err();

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

/// Writes a note and then its open index entry through the adapter ports.
fn generic_add(store: &ObsidianStore, new: NewTask) -> Result<TaskRecord, ObsidianStoreError> {
    let project = foo_project(store);
    let section = new.section.clone();
    let record = insert_next(store, &project, new)?;
    let id = record.id.clone();
    IndexEntryStore::upsert_index_entry(
        store,
        &project,
        IndexEntry {
            id,
            state: IndexEntryState::Open,
            section,
        },
    )?;
    Ok(record)
}

fn insert_next(
    store: &ObsidianStore,
    project: &Project,
    new: NewTask,
) -> Result<TaskRecord, ObsidianStoreError> {
    let id = TaskStore::next_id(store, project)?;
    TaskStore::insert(store, project, &id, new)
}

fn new_task(body: &str, title: &str, section: Option<&str>) -> NewTask {
    NewTask {
        body: body.to_string(),
        title: TaskTitle::try_new(title).unwrap(),
        created_at: task_timestamp("2026-07-07T12:34:56Z"),
        section: section.map(task_section),
        blocked_by: None,
        effort: None,
        priority: None,
        tags: None,
    }
}

#[test]
fn generic_add_creates_note_and_links_index() {
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
            ..new_task(
                "## Goals\n\n## Done When\n\n- tests pass",
                "ship adapter",
                Some("Human"),
            )
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
    assert!(note.contains("created_at: 2026-07-07T12:34:56Z"), "{note}");
    assert!(note.contains("blocked_by: [\"[[FOO-0001]]\"]"), "{note}");
    assert!(note.contains("effort: medium"), "{note}");
    assert!(note.contains("priority: highest"), "{note}");
    let expected_body = format!("## Goals{S}## Done When{S}- tests pass");
    assert!(note.contains(&expected_body), "{note}");
    let index = std::fs::read_to_string(notes_dir.join("foo/foo.md")).unwrap();
    assert_eq!(
        index,
        "---\nid: foo\ntitle: foo\n---\n\n## Human\n\n- [ ] [[FOO-0001]]\n"
    );
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
            ..new_task("tagged task", "tagged task", None)
        },
    )
    .unwrap();
    let note = std::fs::read_to_string(tagged.locator.as_path()).unwrap();
    assert!(note.contains("tags: [sqlite, csharp_export]\n"), "{note}");

    let untagged = generic_add(&store, new_task("untagged task", "untagged task", None)).unwrap();
    let note = std::fs::read_to_string(untagged.locator.as_path()).unwrap();
    assert!(!note.contains("tags:"), "{note}");
}

#[test]
fn generic_insert_rejects_unreadable_existing_index_before_writing_a_note() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(project_dir.join("foo.md")).unwrap();
    let store = store_with_index_identity(&project_dir);
    let project = foo_project(&store);

    let err = insert_next(
        &store,
        &project,
        new_task("Ship the adapter /d tests pass", "ship adapter", None),
    )
    .unwrap_err();

    assert!(err.to_string().starts_with("Cannot read index: "));
    assert!(
        !project_dir.join("FOO-0001.md").exists(),
        "the failed insert must not leave a note behind"
    );
}

#[test]
fn generic_insert_rejects_mismatched_project_index_identity() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("foo.md"),
        "---\nid: smp\ntitle: sample-project\n---\n\n# wrong\n",
    )
    .unwrap();
    let store = store_with_index_identity(&notes_dir.join("foo"));
    let project = foo_project(&store);

    let error = insert_next(&store, &project, new_task("task", "task", None)).unwrap_err();

    assert_matches!(
        error,
        ObsidianStoreError::ProjectIndexIdentityMismatch { .. }
    );
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

    let record = insert_next(&store, &project, new_task("next task", "next task", None)).unwrap();

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

    let error =
        insert_next(&store, &project, new_task("next task", "next task", None)).unwrap_err();

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
    let record = TaskStore::get(store, project, id).unwrap().unwrap();
    let writes = TaskWriteSet::try_new(
        vec![ExpectedTaskRevision {
            id: id.clone(),
            revision: record.revision,
        }],
        writes,
    )
    .unwrap();
    TaskMutationStore::commit_task_writes(store, project, writes).unwrap();
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
fn generic_delete_moves_note_to_vault_trash_and_unlinks_index() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::create_dir(notes_dir.join(".obsidian")).unwrap();
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
    let project = foo_project(&store);
    let id = TaskId::try_new("FOO-0001").unwrap();

    commit_for_task(
        &store,
        &project,
        &id,
        vec![
            TaskWrite::DeleteIndex(id.clone()),
            TaskWrite::MoveToTrash { id: id.clone() },
        ],
    );

    assert!(!task_path.exists());
    assert_eq!(
        std::fs::read_to_string(notes_dir.join(".trash/FOO-0001.md")).unwrap(),
        "---\nid: FOO-0001\nstatus: active\ntitle: stale task\nproject: foo\ncreated_at: 2026-07-01T00:00:00Z\n---\n\nremove me\n"
    );
    assert_eq!(
        std::fs::read_to_string(project_dir.join("foo.md")).unwrap(),
        "---\nid: foo\ntitle: foo\n---\n"
    );
}

#[test]
fn note_backed_record_revision_hashes_the_complete_persisted_note() {
    let staged = staged_open_task(None, "body with exact bytes\r\n");
    let project = foo_project(&staged.store);
    let id = TaskId::try_new("FOO-0001").unwrap();

    let record = TaskStore::get(&staged.store, &project, &id)
        .unwrap()
        .unwrap();

    assert_eq!(
        record.revision,
        content_revision(&std::fs::read(&staged.task_path).unwrap())
    );
}

#[test]
fn missing_note_record_revision_hashes_the_complete_project_index() {
    let temp = tempfile::tempdir().unwrap();
    let project_dir = temp.path().join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    let index_path = project_dir.join("foo.md");
    std::fs::write(
        &index_path,
        "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0001|missing]]\n<!-- retained -->\n",
    )
    .unwrap();
    let store = store_for_tasks(&project_dir);
    let project = foo_project(&store);
    let id = TaskId::try_new("FOO-0001").unwrap();

    let record = TaskStore::get(&store, &project, &id).unwrap().unwrap();

    assert_eq!(
        record.revision,
        content_revision(&std::fs::read(index_path).unwrap())
    );
}

#[test]
fn index_backed_task_writes_share_one_index_snapshot_and_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let project_dir = temp.path().join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    let index_path = project_dir.join("foo.md");
    std::fs::write(
        &index_path,
        "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0001|first]]\n- [ ] [[FOO-0002|second]]\n",
    )
    .unwrap();
    let store = store_for_tasks(&project_dir);
    let project = foo_project(&store);
    let first_id = TaskId::try_new("FOO-0001").unwrap();
    let second_id = TaskId::try_new("FOO-0002").unwrap();
    let first = TaskStore::get(&store, &project, &first_id)
        .unwrap()
        .unwrap();
    let second = TaskStore::get(&store, &project, &second_id)
        .unwrap()
        .unwrap();
    let writes = TaskWriteSet::try_new(
        vec![
            ExpectedTaskRevision {
                id: first_id.clone(),
                revision: first.revision,
            },
            ExpectedTaskRevision {
                id: second_id.clone(),
                revision: second.revision,
            },
        ],
        vec![
            TaskWrite::DeleteIndex(first_id),
            TaskWrite::DeleteIndex(second_id),
        ],
    )
    .unwrap();

    TaskMutationStore::commit_task_writes(&store, &project, writes).unwrap();

    let updated = std::fs::read_to_string(index_path).unwrap();
    assert!(!updated.contains("FOO-0001"));
    assert!(!updated.contains("FOO-0002"));
    assert!(updated.contains("id: foo"));
}

#[test]
fn stale_note_patch_preserves_the_external_edit_and_index() {
    let staged = staged_open_task(None, "original body");
    let project = foo_project(&staged.store);
    let id = TaskId::try_new("FOO-0001").unwrap();
    let record = TaskStore::get(&staged.store, &project, &id)
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
                title: Some(TaskTitle::try_new("local edit").unwrap()),
                ..TaskPatch::default()
            },
        }],
    )
    .unwrap();

    let error = TaskMutationStore::commit_task_writes(&staged.store, &project, writes).unwrap_err();

    assert!(matches!(error, TaskMutationError::StaleTask { .. }));
    assert_eq!(std::fs::read_to_string(staged.task_path).unwrap(), external);
    assert_eq!(std::fs::read(index_path).unwrap(), index_before);
}

#[test]
fn unrelated_index_edit_stales_an_index_backed_task() {
    let temp = tempfile::tempdir().unwrap();
    let project_dir = temp.path().join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    let index_path = project_dir.join("foo.md");
    std::fs::write(
        &index_path,
        "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0001|missing]]\n- [ ] [[FOO-0002|other]]\n",
    )
    .unwrap();
    let store = store_for_tasks(&project_dir);
    let project = foo_project(&store);
    let id = TaskId::try_new("FOO-0001").unwrap();
    let record = TaskStore::get(&store, &project, &id).unwrap().unwrap();
    let external = format!(
        "{}<!-- unrelated edit -->\n",
        std::fs::read_to_string(&index_path).unwrap()
    );
    std::fs::write(&index_path, &external).unwrap();
    let writes = TaskWriteSet::try_new(
        vec![ExpectedTaskRevision {
            id: id.clone(),
            revision: record.revision,
        }],
        vec![TaskWrite::Patch {
            id,
            patch: TaskPatch {
                status: Some(TaskStatus::Done),
                ..TaskPatch::default()
            },
        }],
    )
    .unwrap();

    let error = TaskMutationStore::commit_task_writes(&store, &project, writes).unwrap_err();

    assert!(matches!(error, TaskMutationError::StaleTask { .. }));
    assert_eq!(std::fs::read_to_string(index_path).unwrap(), external);
}

#[test]
fn note_only_patch_does_not_require_the_project_index() {
    let staged = staged_open_task(None, "original body");
    let project = foo_project(&staged.store);
    let id = TaskId::try_new("FOO-0001").unwrap();
    let record = TaskStore::get(&staged.store, &project, &id)
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
                title: Some(TaskTitle::try_new("updated without index").unwrap()),
                ..TaskPatch::default()
            },
        }],
    )
    .unwrap();

    TaskMutationStore::commit_task_writes(&staged.store, &project, writes).unwrap();

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
    let record = TaskStore::get(&store, &project, &id).unwrap().unwrap();
    let index_before = std::fs::read(&index_path).unwrap();
    let external = format!("{}\nexternal edit\n", record.source);
    std::fs::write(&task_path, &external).unwrap();
    let writes = TaskWriteSet::try_new(
        vec![ExpectedTaskRevision {
            id: id.clone(),
            revision: record.revision,
        }],
        vec![
            TaskWrite::DeleteIndex(id.clone()),
            TaskWrite::MoveToTrash { id },
        ],
    )
    .unwrap();

    let error = TaskMutationStore::commit_task_writes(&store, &project, writes).unwrap_err();

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
    ensure_test_index_identity(&tasks_path.join("foo.md"), "foo", "foo");
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
    let record = TaskStore::get(&store, &project, &id).unwrap().unwrap();

    assert_eq!(record.id, id);
    assert_eq!(record.materialization, Materialization::NoteFile);
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
    assert_eq!(record.section, None);
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

    let error =
        TaskStore::get(&store, &project, &TaskId::try_new("FOO-0001").unwrap()).unwrap_err();

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

    let error =
        TaskStore::get(&store, &project, &TaskId::try_new("FOO-0001").unwrap()).unwrap_err();

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
fn task_record_materializes_index_entry_without_a_note() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    let index_path = project_dir.join("foo.md");
    std::fs::write(
        &index_path,
        "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0002|handle it]]\n",
    )
    .unwrap();
    let store = store_for_tasks(&notes_dir.join("foo"));

    let project = foo_project(&store);
    let id = TaskId::try_new("FOO-0002").unwrap();
    let record = TaskStore::get(&store, &project, &id).unwrap().unwrap();

    let expected_note = project_dir.join("FOO-0002.md");
    assert_eq!(record.id, id);
    assert_eq!(record.title, "handle it");
    assert_eq!(record.status, TaskStatus::Active);
    assert_eq!(record.created_at, None);
    assert_eq!(record.completed_at, None);
    assert_eq!(record.section, None);
    assert_eq!(record.locator.as_path(), expected_note);
    assert_eq!(record.source, "");
    assert_eq!(record.body, "");
    assert_eq!(
        record.materialization,
        Materialization::MissingNote {
            expected: TaskNotePath::new(expected_note)
        }
    );
}

#[test]
fn index_entries_parse_open_done_and_raw_futuro_section() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("foo.md"),
        concat!(
            "---\nid: foo\ntitle: foo\n---\n\n",
            "- [ ] [[FOO-0001]]\n",
            "- [x] [[FOO-0002]] \u{2705} 2026-07-02\n",
            "\n## Futuro\n",
            "- [ ] [[FOO-0003]]\n",
        ),
    )
    .unwrap();
    let store = store_for_tasks(&notes_dir.join("foo"));

    let project = foo_project(&store);
    let entries = IndexEntryStore::list_index_entries(&store, &project).unwrap();

    assert_eq!(
        entries,
        vec![
            IndexEntry {
                id: TaskId::try_new("FOO-0001").unwrap(),
                state: IndexEntryState::Open,
                section: None,
            },
            IndexEntry {
                id: TaskId::try_new("FOO-0002").unwrap(),
                state: IndexEntryState::Done(Some(task_timestamp("2026-07-02T00:00:00Z",))),
                section: None,
            },
            IndexEntry {
                id: TaskId::try_new("FOO-0003").unwrap(),
                state: IndexEntryState::Open,
                section: Some(task_section("Futuro")),
            },
        ]
    );
}

#[test]
fn patch_status_done_closes_index_entry_without_a_date_stamp() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    let index_path = project_dir.join("foo.md");
    std::fs::write(
        &index_path,
        "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0002|handle it]]\n",
    )
    .unwrap();
    let store = store_for_tasks(&notes_dir.join("foo"));

    let project = foo_project(&store);
    let id = TaskId::try_new("FOO-0002").unwrap();
    let patch = TaskPatch {
        status: Some(TaskStatus::Done),
        completed_at: NullablePatch::Set(task_timestamp("2026-07-15T12:34:56Z")),
        ..Default::default()
    };

    commit_for_task(
        &store,
        &project,
        &id,
        vec![TaskWrite::Patch {
            id: id.clone(),
            patch,
        }],
    );

    let index = std::fs::read_to_string(&index_path).unwrap();
    assert!(
        index.contains("- [x] [[FOO-0002|handle it]]"),
        "checkbox not flipped; got:\n{index}"
    );
    assert!(!index.contains('\u{2705}'), "date stamp retained:\n{index}");
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
            body: "wire up the new thing".to_string(),
            title: TaskTitle::try_new("wire up the new thing").unwrap(),
            created_at: task_timestamp("2026-07-15T12:34:56Z"),
            section: None,
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
    let id = TaskStore::next_id(&store, &project).unwrap();
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

    let error = TaskStore::insert(
        &store,
        &project,
        &id,
        new_task("replacement", "replacement", None),
    )
    .unwrap_err();

    assert_matches!(
        error,
        ObsidianStoreError::TaskIdOccupied { ref id, path: ref error_path }
            if id.as_ref() == "FOO-0001" && error_path == &path
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), before);
}

/// Verifies that open index links contribute placement without owning list membership.
#[test]
fn generic_list_records_carry_open_placement_without_hiding_unlinked_notes() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    let index_path = project_dir.join("foo.md");
    std::fs::write(
        &index_path,
        concat!(
            "---\nid: foo\ntitle: foo\n---\n\n",
            "## Futuro\n",
            "- [ ] [[FOO-0001|linked]]\n",
        ),
    )
    .unwrap();
    write_note(
        &project_dir.join("FOO-0001.md"),
        "linked",
        "2026-07-01",
        None,
        None,
        None,
        "body",
    );
    write_note(
        &project_dir.join("FOO-0002.md"),
        "unlinked",
        "2026-07-01",
        None,
        None,
        None,
        "body",
    );
    let store = store_for_tasks(&notes_dir.join("foo"));

    let project = foo_project(&store);
    let records = TaskStore::list(&store, &project).unwrap();

    assert_eq!(records.len(), 2);
    let record = records
        .iter()
        .find(|record| record.id.as_ref() == "FOO-0001")
        .unwrap();
    assert_eq!(record.id, TaskId::try_new("FOO-0001").unwrap());
    assert_eq!(
        record.placement,
        Some(IndexPlacement {
            index_path: TaskIndexPath::new(index_path.clone()),
            line: NonZeroUsize::new(7).unwrap(),
        })
    );
    assert_eq!(record.section.as_ref().map(AsRef::as_ref), Some("Futuro"));
    let unlinked = records
        .iter()
        .find(|record| record.id.as_ref() == "FOO-0002")
        .unwrap();
    assert!(unlinked.placement.is_none());
    assert_eq!(unlinked.section, None);
}

#[test]
fn generic_list_rejects_duplicate_project_index_task_ids() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    let index_path = project_dir.join("foo.md");
    std::fs::write(
        &index_path,
        concat!(
            "---\nid: foo\ntitle: foo\n---\n\n",
            "- [ ] [[FOO-0001|first]]\n",
            "## Human\n",
            "- [ ] [[FOO-0001|duplicate]]\n",
        ),
    )
    .unwrap();
    write_note(
        &project_dir.join("FOO-0001.md"),
        "task",
        "2026-07-18",
        None,
        None,
        None,
        "body",
    );
    let store = store_for_tasks(&notes_dir.join("foo"));
    let project = foo_project(&store);

    let error = TaskStore::list(&store, &project).unwrap_err();

    assert_matches!(
        error,
        ObsidianStoreError::ProjectIndexTaskIdDuplicate {
            path,
            id,
            lines,
        } if path == index_path && id.as_ref() == "FOO-0001" && lines == [6, 8]
    );
}

#[test]
fn list_tasks_returns_note_history_and_index_only_records() {
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

    let records = TaskStore::list(&store, &project).unwrap();
    let mut ids: Vec<String> = records.iter().map(|record| record.id.to_string()).collect();
    ids.sort();

    assert_eq!(
        ids,
        [
            "FOO-0001", "FOO-0002", "FOO-0003", "FOO-0004", "FOO-0005", "FOO-0006",
        ]
    );
    let record = |id: &str| {
        records
            .iter()
            .find(|record| record.id.as_ref() == id)
            .unwrap()
    };
    assert_eq!(
        record("FOO-0001").placement,
        Some(IndexPlacement {
            index_path: TaskIndexPath::new(index_path.clone()),
            line: NonZeroUsize::new(6).unwrap(),
        })
    );
    assert_eq!(record("FOO-0002").status, TaskStatus::Done);
    assert_eq!(
        record("FOO-0002").section.as_ref().map(AsRef::as_ref),
        Some("Human")
    );
    assert!(record("FOO-0002").placement.is_none());
    assert_eq!(record("FOO-0003").status, TaskStatus::Cancelled);
    assert_eq!(record("FOO-0004").status, TaskStatus::Done);
    assert!(record("FOO-0004").placement.is_none());
    assert_eq!(record("FOO-0005").status, TaskStatus::Active);
    assert!(record("FOO-0005").placement.is_none());
    let missing_done = record("FOO-0006");
    assert_eq!(missing_done.status, TaskStatus::Done);
    assert_matches!(
        &missing_done.materialization,
        Materialization::MissingNote { .. }
    );
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

    let records = TaskStore::list(&store, &project).unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TaskStatus::Done);
    assert!(records[0].placement.is_none());
}

#[test]
fn index_sections_list_raw_h2_labels_in_document_order() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("foo.md"),
        concat!(
            "---\nid: foo\ntitle: foo\n---\n\n",
            "- [ ] [[FOO-0001|alias]]\n\n",
            "## Human\n- [ ] [[FOO-0002|h]]\n\n",
            "## Futuro\n\n",
            "## Low-prio\n\n",
            "### Notes\n- [[FOO-NOTE-0001]]\n",
        ),
    )
    .unwrap();
    let store = store_for_tasks(&notes_dir.join("foo"));
    let project = foo_project(&store);

    let sections = IndexSectionStore::list_index_sections(&store, &project).unwrap();

    // RAW labels in document order; H3 regions (### Notes) are not sections.
    assert_eq!(
        sections,
        ["Human", "Futuro", "Low-prio"].map(task_section).to_vec()
    );
}

#[test]
fn index_sections_list_empty_when_index_missing() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    std::fs::create_dir_all(notes_dir.join("foo")).unwrap();
    let store = store_for_tasks(&notes_dir.join("foo"));
    let project = foo_project(&store);

    let sections = IndexSectionStore::list_index_sections(&store, &project).unwrap();

    assert!(sections.is_empty());
}

/// Verifies that section update renames an H2 label in place.
#[test]
fn index_section_update_renames_header_in_place() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("foo.md"),
        "---\nid: foo\ntitle: foo\n---\n\n## Futuro\n\n- [ ] [[FOO-0001]]\n",
    )
    .unwrap();
    let store = store_for_tasks(&notes_dir.join("foo"));
    let project = foo_project(&store);

    IndexSectionStore::rename_index_section(
        &store,
        &project,
        &task_section("Futuro"),
        &TaskSection::future(),
    )
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(project_dir.join("foo.md")).unwrap(),
        "---\nid: foo\ntitle: foo\n---\n\n## Future\n\n- [ ] [[FOO-0001]]\n"
    );
}

/// Captures one add scenario and its expected byte-exact index.
struct AddParityScenario {
    name: &'static str,
    initial_index: Option<&'static str>,
    section: Option<&'static str>,
    expected_index: &'static str,
}

const PARITY_IDENTITY: &str = "---\nid: foo\ntitle: foo\n---\n\n";

fn add_parity_scenarios() -> Vec<AddParityScenario> {
    vec![
        AddParityScenario {
            name: "general section with existing anchor",
            initial_index: Some("---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0001|tray gui]]\n"),
            section: None,
            expected_index: "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0002]]\n- [ ] [[FOO-0001|tray gui]]\n",
        },
        AddParityScenario {
            name: "human section created before existing future",
            initial_index: Some(
                "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0001|tray gui]]\n\n## Future\n\n- [ ] [[FOO-0009|later]]\n",
            ),
            section: Some("Human"),
            expected_index: "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0001|tray gui]]\n\n## Human\n\n- [ ] [[FOO-0002]]\n## Future\n\n- [ ] [[FOO-0009|later]]\n",
        },
        AddParityScenario {
            name: "existing empty human header is not re-created",
            initial_index: Some(
                "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0001|tray gui]]\n\n## Human\n",
            ),
            section: Some("Human"),
            expected_index: "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0001|tray gui]]\n\n## Human\n- [ ] [[FOO-0002]]\n",
        },
        AddParityScenario {
            name: "future lands under legacy futuro alias header",
            initial_index: Some(
                "---\nid: foo\ntitle: foo\n---\n\n## Futuro\n\n- [ ] [[FOO-0009|later]]\n",
            ),
            section: Some("Future"),
            expected_index: "---\nid: foo\ntitle: foo\n---\n\n## Futuro\n- [ ] [[FOO-0002]]\n\n- [ ] [[FOO-0009|later]]\n",
        },
        AddParityScenario {
            name: "low-prio section created at end",
            initial_index: Some("---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0001|tray gui]]\n"),
            section: Some("Low-prio"),
            expected_index: "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0001|tray gui]]\n\n## Low-prio\n\n- [ ] [[FOO-0002]]\n",
        },
        AddParityScenario {
            name: "fresh vault creates identity template",
            initial_index: None,
            section: None,
            expected_index: "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0002]]\n",
        },
        AddParityScenario {
            name: "fresh vault with section creates template and section",
            initial_index: None,
            section: Some("Human"),
            expected_index: "---\nid: foo\ntitle: foo\n---\n\n## Human\n\n- [ ] [[FOO-0002]]\n",
        },
        AddParityScenario {
            name: "general add stays above trailing notes block",
            initial_index: Some(
                "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0001|tray gui]]\n\n### Notes\n- [[FOO-NOTE-0001]]\n",
            ),
            section: None,
            expected_index: "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0002]]\n- [ ] [[FOO-0001|tray gui]]\n\n### Notes\n- [[FOO-NOTE-0001]]\n",
        },
    ]
}

fn stage_add_parity_vault(
    initial_index: Option<&str>,
) -> (tempfile::TempDir, ObsidianStore, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    write_note(
        &project_dir.join("FOO-0001.md"),
        "tray gui",
        "2026-01-01",
        None,
        None,
        None,
        "add toggle",
    );
    if let Some(index) = initial_index {
        std::fs::write(project_dir.join("foo.md"), index).unwrap();
    }
    let store = store_for_tasks(&notes_dir.join("foo"));
    (temp, store, project_dir)
}

/// Verifies byte-exact section placement, H3 safety, link format, and fresh-index creation.
#[test]
fn generic_insert_and_upsert_preserve_index_placement_bytes() {
    for scenario in add_parity_scenarios() {
        let (_guard, store, project_dir) = stage_add_parity_vault(scenario.initial_index);
        let project = foo_project(&store);

        let record = insert_next(
            &store,
            &project,
            NewTask {
                body: "do the thing".to_string(),
                title: TaskTitle::try_new("ship it").unwrap(),
                created_at: task_timestamp("2026-07-07T12:34:56Z"),
                section: scenario.section.map(task_section),
                blocked_by: None,
                effort: None,
                priority: None,
                tags: None,
            },
        )
        .unwrap();
        let id = record.id.clone();
        assert_eq!(
            id.as_ref(),
            "FOO-0002",
            "allocated id for `{}`",
            scenario.name
        );
        IndexEntryStore::upsert_index_entry(
            &store,
            &project,
            IndexEntry {
                id,
                state: IndexEntryState::Open,
                section: scenario.section.map(task_section),
            },
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(project_dir.join("foo.md")).unwrap(),
            scenario.expected_index,
            "index bytes for scenario `{}`",
            scenario.name
        );
        assert!(
            project_dir.join("FOO-0002.md").exists(),
            "note file for scenario `{}`",
            scenario.name
        );
    }
}

#[test]
fn upsert_creates_missing_index_from_identity_template() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    std::fs::create_dir_all(notes_dir.join("foo")).unwrap();
    let store = store_for_tasks(&notes_dir.join("foo"));
    let project = foo_project(&store);

    IndexEntryStore::upsert_index_entry(
        &store,
        &project,
        IndexEntry {
            id: TaskId::try_new("FOO-0001").unwrap(),
            state: IndexEntryState::Open,
            section: None,
        },
    )
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(notes_dir.join("foo/foo.md")).unwrap(),
        "---\nid: foo\ntitle: foo\n---\n\n- [ ] [[FOO-0001]]\n"
    );
}

#[test]
fn delete_index_entry_is_idempotent_when_no_link_matches() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("foo.md"), PARITY_IDENTITY).unwrap();
    let store = store_for_tasks(&notes_dir.join("foo"));
    let project = foo_project(&store);

    IndexEntryStore::delete_index_entry(&store, &project, &TaskId::try_new("FOO-0002").unwrap())
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(project_dir.join("foo.md")).unwrap(),
        PARITY_IDENTITY
    );
}

#[test]
fn delete_index_entry_is_idempotent_when_index_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("foo");
    std::fs::create_dir_all(&project_dir).unwrap();
    let store = store_for_tasks(&project_dir);
    let project = foo_project(&store);

    IndexEntryStore::delete_index_entry(&store, &project, &TaskId::try_new("FOO-0002").unwrap())
        .unwrap();

    assert!(!project_dir.join("foo.md").exists());
}
