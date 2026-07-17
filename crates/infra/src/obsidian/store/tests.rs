use std::{assert_matches, fmt::Write as _, path::Path};

use pwf_application::{
    AppDbStore, IndexEntry, IndexEntryState, IndexPlacement, IndexSection, ItemPatch,
    Materialization, NewItem, PendingWorkItem, RecordId,
};
use pwf_core::config::from_json;
use pwf_domain::pending_work::{ProjectName, Tags, Timestamp, WorkItemId, WorkItemStatus};

use super::{ObsidianStore, ObsidianStoreError, fs::path_str};

#[test]
fn generic_list_rejects_project_index_without_identity_frontmatter() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("pwf.md"), "- [ ] [[PWF-0001]]\n").unwrap();
    write_note(
        &project_dir.join("PWF-0001.md"),
        "task",
        "2026-07-12",
        None,
        None,
        None,
        "body",
    );
    let store = ObsidianStore::new(raw_config_for_notes(&notes_dir));

    let project = ProjectName::try_new("pwf").unwrap();
    let error = <ObsidianStore as AppDbStore<PendingWorkItem>>::list(&store, &project).unwrap_err();

    assert_matches!(
        error,
        ObsidianStoreError::MissingFrontmatter { property, .. }
            if property == "id/title"
    );
}

#[test]
fn generic_read_retains_raw_tags_frontmatter() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("pwf.md"), "- [ ] [[PWF-0001]]\n").unwrap();
    write_note(
        &project_dir.join("PWF-0001.md"),
        "tagged",
        "2026-07-01",
        None,
        None,
        Some("[SQLite, malformed-but-displayable]"),
        "body",
    );
    let store = ObsidianStore::new(config_for_notes(&notes_dir));

    assert_eq!(
        get_record(&store, "PWF-0001").unwrap().tags.as_deref(),
        Some("[SQLite, malformed-but-displayable]")
    );
}

#[test]
fn generic_read_uses_yaml_decoded_title() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("pwf.md"), "- [ ] [[PWF-0001]]\n").unwrap();
    std::fs::write(
        project_dir.join("PWF-0001.md"),
        "---\nid: PWF-0001\nstatus: active\ntitle: \"adapter: preserve identity\"\nproject: pwf\ncreated: 2026-07-12\n---\n\nbody\n",
    )
    .unwrap();
    let store = ObsidianStore::new(config_for_notes(&notes_dir));

    let record = get_record(&store, "PWF-0001").unwrap();

    assert_eq!(record.title, "adapter: preserve identity");
}

fn get_record(store: &ObsidianStore, id: &str) -> Option<PendingWorkItem> {
    let project = ProjectName::try_new("pwf").unwrap();
    <ObsidianStore as AppDbStore<PendingWorkItem>>::get(
        store,
        &project,
        &WorkItemId::try_new(id).unwrap(),
    )
    .unwrap()
}

#[test]
fn get_resolves_frontmatter_id_to_descriptive_filename_locator() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("pwf.md"), "- [ ] [[PWF-0001]]\n").unwrap();
    std::fs::write(
        project_dir.join("descriptive-name.md"),
        "---\nid: PWF-0001\nstatus: active\ntitle: descriptive\nproject: pwf\ncreated: 2026-07-12\n---\n\nbody\n",
    )
    .unwrap();
    let store = ObsidianStore::new(config_for_notes(&notes_dir));

    let record = get_record(&store, "PWF-0001").expect("record must resolve by frontmatter id");

    assert_eq!(
        record.locator,
        path_str(&project_dir.join("descriptive-name.md"))
    );
}

#[test]
fn get_rejects_duplicate_frontmatter_ids() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("pwf.md"), "- [ ] [[PWF-0001]]\n").unwrap();
    for (filename, id) in [
        ("a.md", "PWF-0001"),
        ("b.md", "PWF-0002"),
        ("c.md", "PWF-0001"),
    ] {
        std::fs::write(
            project_dir.join(filename),
            format!("---\nid: {id}\nstatus: active\ntitle: task\nproject: pwf\ncreated: 2026-07-12\n---\n\nbody\n"),
        )
        .unwrap();
    }
    let store = ObsidianStore::new(config_for_notes(&notes_dir));
    let project = ProjectName::try_new("pwf").unwrap();

    let error = <ObsidianStore as AppDbStore<PendingWorkItem>>::get(
        &store,
        &project,
        &WorkItemId::try_new("PWF-0001").unwrap(),
    )
    .unwrap_err();

    assert_matches!(
        error,
        ObsidianStoreError::DuplicateTaskId { ref id, .. } if id == "PWF-0001"
    );
}

fn write_note(
    path: &Path,
    title: &str,
    created: &str,
    prereq: Option<&str>,
    effort: Option<&str>,
    tags: Option<&str>,
    body: &str,
) {
    let id = path.file_stem().and_then(|stem| stem.to_str()).unwrap();
    let mut note = format!(
        "---\nid: {id}\nstatus: active\ntitle: {title}\nproject: pwf\ncreated: {created}\n"
    );
    if let Some(prereq) = prereq {
        let _ = writeln!(note, "prereq: {prereq}");
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
fn generic_add(store: &ObsidianStore, new: NewItem) -> Result<PendingWorkItem, ObsidianStoreError> {
    let project = ProjectName::try_new("pwf").unwrap();
    let section = new.section.clone().unwrap_or_default();
    let record = <ObsidianStore as AppDbStore<PendingWorkItem>>::insert(store, &project, new)?;
    let id = record
        .id
        .as_item()
        .expect("inserted record carries a canonical id")
        .clone();
    <ObsidianStore as AppDbStore<IndexEntry>>::insert(
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

fn new_item(prompt: &str, title: Option<&str>, section: Option<&str>) -> NewItem {
    NewItem {
        prompt: prompt.to_string(),
        title: title.map(str::to_string),
        created: Timestamp::new("2026-07-07"),
        section: section.map(str::to_string),
        prereq: None,
        effort: None,
        tags: None,
    }
}

#[test]
fn generic_add_creates_note_and_links_index() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let config = config_for_notes(&notes_dir);
    let store = ObsidianStore::new(config);

    let record = generic_add(
        &store,
        NewItem {
            prereq: Some("[[PWF-0001]]".to_string()),
            effort: Some(2),
            ..new_item(
                "Ship the adapter /d tests pass",
                Some("Ship Adapter"),
                Some("Human"),
            )
        },
    )
    .unwrap();

    assert_eq!(
        record.id,
        RecordId::Item(WorkItemId::try_new("PWF-0001").unwrap())
    );
    assert_eq!(record.title, "ship adapter");
    let note = std::fs::read_to_string(notes_dir.join("pwf/PWF-0001.md")).unwrap();
    assert!(
        note.starts_with("---\nid: PWF-0001\nstatus: active\n"),
        "{note}"
    );
    assert!(note.contains("status: active"), "{note}");
    assert!(note.contains("title: ship adapter"), "{note}");
    assert!(note.contains("created: 2026-07-07"), "{note}");
    assert!(note.contains("prereq: \"[[PWF-0001]]\""), "{note}");
    assert!(note.contains("effort: 2"), "{note}");
    assert!(note.contains("## Goals\n- Ship the adapter"), "{note}");
    let index = std::fs::read_to_string(notes_dir.join("pwf/pwf.md")).unwrap();
    assert_eq!(
        index,
        "---\nid: pwf\ntitle: pwf\n---\n\n## Human\n\n- [ ] [[PWF-0001]]\n"
    );
}

#[test]
fn generic_add_writes_canonical_tags_and_omits_absent_tags() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let store = ObsidianStore::new(config_for_notes(&notes_dir));
    let tags = Tags::parse_values(&["SQLite,csharp-export".to_string()]).unwrap();
    let tagged = generic_add(
        &store,
        NewItem {
            tags: Some(tags),
            ..new_item("tagged task", None, None)
        },
    )
    .unwrap();
    let note = std::fs::read_to_string(tagged.locator).unwrap();
    assert!(note.contains("tags: [sqlite, csharp_export]\n"), "{note}");

    let untagged = generic_add(&store, new_item("untagged task", None, None)).unwrap();
    let note = std::fs::read_to_string(untagged.locator).unwrap();
    assert!(!note.contains("tags:"), "{note}");
}

#[test]
fn generic_insert_rejects_unreadable_existing_index_before_writing_a_note() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(project_dir.join("pwf.md")).unwrap();
    let config = config_for_notes(&notes_dir);
    let store = ObsidianStore::new(config);
    let project = ProjectName::try_new("pwf").unwrap();

    let err = <ObsidianStore as AppDbStore<PendingWorkItem>>::insert(
        &store,
        &project,
        new_item("Ship the adapter /d tests pass", Some("Ship Adapter"), None),
    )
    .unwrap_err();

    assert!(err.to_string().starts_with("Cannot read index: "));
    assert!(
        !project_dir.join("PWF-0001.md").exists(),
        "the failed insert must not leave a note behind"
    );
}

#[test]
fn generic_insert_rejects_mismatched_project_index_identity() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("pwf.md"),
        "---\nid: rst\ntitle: rust-learn\n---\n\n# wrong\n",
    )
    .unwrap();
    let store = ObsidianStore::new(config_for_notes(&notes_dir));
    let project = ProjectName::try_new("pwf").unwrap();

    let error = <ObsidianStore as AppDbStore<PendingWorkItem>>::insert(
        &store,
        &project,
        new_item("task", None, None),
    )
    .unwrap_err();

    assert_matches!(
        error,
        ObsidianStoreError::ProjectIndexIdentityMismatch { .. }
    );
}

#[test]
fn generic_insert_allocates_after_greatest_frontmatter_id() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("pwf.md"),
        "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0009]]\n",
    )
    .unwrap();
    std::fs::write(
        project_dir.join("descriptive.md"),
        "---\nid: PWF-0009\nstatus: active\ntitle: existing\nproject: pwf\ncreated: 2026-07-12\n---\n\nbody\n",
    )
    .unwrap();
    std::fs::write(
        project_dir.join("PWF-0099.md"),
        "---\ntype: note\n---\n\nnote\n",
    )
    .unwrap();
    let store = ObsidianStore::new(config_for_notes(&notes_dir));
    let project = ProjectName::try_new("pwf").unwrap();

    let record = <ObsidianStore as AppDbStore<PendingWorkItem>>::insert(
        &store,
        &project,
        new_item("next task", None, None),
    )
    .unwrap();

    assert_eq!(
        record.id,
        RecordId::Item(WorkItemId::try_new("PWF-0010").unwrap())
    );
}

/// Applies a tags-only patch; `Some(tags)` sets the field and `None` clears it.
fn apply_tag_patch(store: &ObsidianStore, tags: Option<Tags>) {
    let project = ProjectName::try_new("pwf").unwrap();
    let id = WorkItemId::try_new("PWF-0001").unwrap();
    let patch = ItemPatch {
        tags: Some(tags),
        ..Default::default()
    };
    <ObsidianStore as AppDbStore<PendingWorkItem>>::update(store, &project, &id, patch).unwrap();
}

fn sqlite_tags() -> Tags {
    Tags::parse_values(&["SQLite".to_string()]).unwrap()
}

#[test]
fn generic_update_sets_frontmatter_tags_without_rewriting_body_tags_line() {
    let body = "tags: body-only value\nkeep this body byte-identical";
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item(None, body);

    apply_tag_patch(&store, Some(sqlite_tags()));

    assert_eq!(
        std::fs::read_to_string(item_path).unwrap(),
        concat!(
            "---\n",
            "id: PWF-0001\n",
            "status: active\n",
            "title: tagged\n",
            "project: pwf\n",
            "created: 2026-07-01\n",
            "tags: [sqlite]\n",
            "---\n\n",
            "tags: body-only value\n",
            "keep this body byte-identical\n",
        )
    );
}

#[test]
fn generic_update_writes_a_deduplicated_tag_value() {
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item_with_tags("[sqlite, godot]");
    let merged = Tags::parse_values(&["sqlite,godot,csharp-export".to_string()]).unwrap();

    apply_tag_patch(&store, Some(merged));

    let note = std::fs::read_to_string(item_path).unwrap();
    assert!(
        note.contains("tags: [sqlite, godot, csharp_export]\n"),
        "{note}"
    );
    assert_eq!(note.matches("tags:").count(), 1);
}

#[test]
fn generic_update_clear_preserves_body_tags_line_when_frontmatter_has_no_tags() {
    let body = "tags: body-only value\nkeep this body byte-identical";
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item(None, body);
    let before = std::fs::read_to_string(&item_path).unwrap();

    apply_tag_patch(&store, None);

    assert_eq!(std::fs::read_to_string(item_path).unwrap(), before);
}

#[test]
fn generic_update_sets_tags_on_bom_frontmatter() {
    let before = formatted_tag_note(true, "\n", None);
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item_from_note(&before);

    apply_tag_patch(&store, Some(sqlite_tags()));

    assert_eq!(
        std::fs::read_to_string(item_path).unwrap(),
        formatted_tag_note(true, "\n", Some("[sqlite]"))
    );
}

#[test]
fn generic_update_clears_tags_on_bom_frontmatter() {
    let before = formatted_tag_note(true, "\n", Some("[godot]"));
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item_from_note(&before);

    apply_tag_patch(&store, None);

    assert_eq!(
        std::fs::read_to_string(item_path).unwrap(),
        formatted_tag_note(true, "\n", None)
    );
}

#[test]
fn generic_update_sets_tags_on_crlf_frontmatter() {
    let before = formatted_tag_note(false, "\r\n", None);
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item_from_note(&before);

    apply_tag_patch(&store, Some(sqlite_tags()));

    assert_eq!(
        std::fs::read_to_string(item_path).unwrap(),
        formatted_tag_note(false, "\r\n", Some("[sqlite]"))
    );
}

#[test]
fn generic_update_clears_tags_on_crlf_frontmatter() {
    let before = formatted_tag_note(false, "\r\n", Some("[godot]"));
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item_from_note(&before);

    apply_tag_patch(&store, None);

    assert_eq!(
        std::fs::read_to_string(item_path).unwrap(),
        formatted_tag_note(false, "\r\n", None)
    );
}

#[test]
fn generic_delete_removes_note_and_unlinks_index() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("pwf.md"), "- [ ] [[PWF-0001]]\n").unwrap();
    write_note(
        &project_dir.join("PWF-0001.md"),
        "stale task",
        "2026-07-01",
        None,
        None,
        None,
        "remove me",
    );
    let store = ObsidianStore::new(config_for_notes(&notes_dir));
    let project = ProjectName::try_new("pwf").unwrap();
    let id = WorkItemId::try_new("PWF-0001").unwrap();

    <ObsidianStore as AppDbStore<IndexEntry>>::delete(&store, &project, &id).unwrap();
    <ObsidianStore as AppDbStore<PendingWorkItem>>::delete(&store, &project, &id).unwrap();

    assert!(!project_dir.join("PWF-0001.md").exists());
    assert_eq!(
        std::fs::read_to_string(project_dir.join("pwf.md")).unwrap(),
        "---\nid: pwf\ntitle: pwf\n---\n"
    );
}

#[test]
fn get_returns_open_note_locator_from_active_index() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("pwf.md"), "- [ ] [[PWF-0001]]\n").unwrap();
    write_note(
        &project_dir.join("PWF-0001.md"),
        "active task",
        "2026-07-01",
        None,
        None,
        None,
        "body",
    );
    let store = ObsidianStore::new(config_for_notes(&notes_dir));

    let record = get_record(&store, "PWF-0001").expect("open item must resolve");

    assert_eq!(record.locator, path_str(&project_dir.join("PWF-0001.md")));
}

#[test]
fn get_returns_open_note_source_with_created_key() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("pwf.md"), "- [ ] [[PWF-0001]]\n").unwrap();
    write_note(
        &project_dir.join("PWF-0001.md"),
        "active task",
        "2026-07-01",
        None,
        None,
        None,
        "## Goals\n- body",
    );
    let store = ObsidianStore::new(config_for_notes(&notes_dir));

    let record = get_record(&store, "PWF-0001").expect("open item must resolve");

    assert_eq!(
        record.source,
        "---\nid: PWF-0001\nstatus: active\ntitle: active task\nproject: pwf\ncreated: 2026-07-01\n---\n\n## Goals\n- body\n"
    );
}

#[test]
fn get_finds_closed_note_still_in_project_dir() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("pwf.md"),
        "- [x] [[PWF-0003]] ✅ 2026-07-07\n",
    )
    .unwrap();
    write_status_note(
        &project_dir.join("PWF-0003.md"),
        "done task",
        "done",
        Some("2026-07-07"),
        None,
    );
    let store = ObsidianStore::new(config_for_notes(&notes_dir));

    let record = get_record(&store, "PWF-0003").expect("closed item must resolve");

    assert_eq!(
        record.source,
        "---\nid: PWF-0003\nstatus: done\ntitle: done task\nproject: pwf\ncreated: 2026-07-01\ncompleted: 2026-07-07\n---\n\nbody\n"
    );
}

#[test]
fn get_finds_closed_note_still_in_project_dir_with_shorthand_id() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("pwf.md"),
        "- [x] [[PWF-0003]] ✅ 2026-07-07\n",
    )
    .unwrap();
    write_status_note(
        &project_dir.join("PWF-0003.md"),
        "done task",
        "done",
        Some("2026-07-07"),
        None,
    );
    let store = ObsidianStore::new(config_for_notes(&notes_dir));

    let record = get_record(&store, "pwf3").expect("shorthand id must resolve");

    assert_eq!(
        record.source,
        "---\nid: PWF-0003\nstatus: done\ntitle: done task\nproject: pwf\ncreated: 2026-07-01\ncompleted: 2026-07-07\n---\n\nbody\n"
    );
}

fn config_for_notes(notes_dir: &Path) -> pwf_core::config::Config {
    ensure_test_index_identity(&notes_dir.join("pwf/pwf.md"), "pwf", "pwf");
    raw_config_for_notes(notes_dir)
}

fn raw_config_for_notes(notes_dir: &Path) -> pwf_core::config::Config {
    let config_json = serde_json::json!({
        "notesDir": notes_dir,
        "projects": { "pwf": "/repo/pwf" },
        "prefixes": { "pwf": "PWF" }
    });
    from_json(&config_json.to_string(), None).unwrap()
}

struct StagedOpenItem {
    _temp: tempfile::TempDir,
    store: ObsidianStore,
    item_path: std::path::PathBuf,
}

fn staged_open_item_with_tags(tags: &str) -> StagedOpenItem {
    staged_open_item(Some(tags), "body")
}

fn staged_open_item(tags: Option<&str>, body: &str) -> StagedOpenItem {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("pwf.md"), "- [ ] [[PWF-0001]]\n").unwrap();
    let item_path = project_dir.join("PWF-0001.md");
    write_note(&item_path, "tagged", "2026-07-01", None, None, tags, body);
    let store = ObsidianStore::new(config_for_notes(&notes_dir));
    StagedOpenItem {
        _temp: temp,
        store,
        item_path,
    }
}

fn staged_open_item_from_note(note: &str) -> StagedOpenItem {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("pwf.md"), "- [ ] [[PWF-0001]]\n").unwrap();
    let item_path = project_dir.join("PWF-0001.md");
    std::fs::write(&item_path, note).unwrap();
    let store = ObsidianStore::new(config_for_notes(&notes_dir));
    StagedOpenItem {
        _temp: temp,
        store,
        item_path,
    }
}

fn formatted_tag_note(bom: bool, newline: &str, tags: Option<&str>) -> String {
    let bom = if bom { "\u{feff}" } else { "" };
    let tags = tags.map_or_else(String::new, |tags| format!("tags: {tags}{newline}"));
    format!(
        "{bom}---{newline}id: PWF-0001{newline}status: active{newline}title: tagged{newline}project: pwf{newline}created: 2026-07-01{newline}{tags}---{newline}{newline}tags: body-only value{newline}keep this body byte-identical{newline}"
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
    completed: Option<&str>,
    commits: Option<&str>,
) {
    let id = path.file_stem().and_then(|stem| stem.to_str()).unwrap();
    let mut note = format!(
        "---\nid: {id}\nstatus: {status}\ntitle: {title}\nproject: pwf\ncreated: 2026-07-01\n"
    );
    if let Some(completed) = completed {
        let _ = writeln!(note, "completed: {completed}");
    }
    if let Some(commits) = commits {
        let _ = writeln!(note, "commits: \"{commits}\"");
    }
    note.push_str("---\n\nbody\n");
    std::fs::write(path, note).unwrap();
}

#[test]
fn item_record_roundtrips_file_model_note() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("pwf.md"), "- [ ] [[PWF-0001]]\n").unwrap();
    let note_path = project_dir.join("PWF-0001.md");
    let source = concat!(
        "---\n",
        "id: PWF-0001\n",
        "status: active\n",
        "title: ship the adapter\n",
        "project: pwf\n",
        "created: 2026-07-01\n",
        "prereq: \"[[CFG-0001]]\"\n",
        "effort: 2\n",
        "tags: [sqlite, godot]\n",
        "---\n",
        "\n",
        "ship the adapter body\n",
    );
    std::fs::write(&note_path, source).unwrap();
    let store = ObsidianStore::new(config_for_notes(&notes_dir));

    let project = ProjectName::try_new("pwf").unwrap();
    let id = WorkItemId::try_new("PWF-0001").unwrap();
    let record = <ObsidianStore as AppDbStore<PendingWorkItem>>::get(&store, &project, &id)
        .unwrap()
        .expect("file-model record present");

    assert_eq!(record.id, RecordId::Item(id));
    assert_eq!(record.materialization, Materialization::NoteFile);
    assert_eq!(record.title, "ship the adapter");
    assert_eq!(record.status, WorkItemStatus::Active);
    assert_eq!(record.created, Some(Timestamp::new("2026-07-01")));
    assert_eq!(record.completed, None);
    assert_eq!(record.commits, None);
    assert_eq!(record.prereq.as_deref(), Some("\"[[CFG-0001]]\""));
    assert_eq!(record.effort.as_deref(), Some("2"));
    assert_eq!(record.tags.as_deref(), Some("[sqlite, godot]"));
    assert_eq!(record.section, None);
    assert_eq!(record.body, "\nship the adapter body\n");
    assert_eq!(record.locator, path_str(&note_path));
    assert_eq!(record.source, source);
}

#[test]
fn item_record_materializes_legacy_checkbox_line() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    let index_path = project_dir.join("pwf.md");
    std::fs::write(
        &index_path,
        "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0002|handle it]]\n",
    )
    .unwrap();
    let store = ObsidianStore::new(raw_config_for_notes(&notes_dir));

    let project = ProjectName::try_new("pwf").unwrap();
    let id = WorkItemId::try_new("PWF-0002").unwrap();
    let record = <ObsidianStore as AppDbStore<PendingWorkItem>>::get(&store, &project, &id)
        .unwrap()
        .expect("legacy checkbox materialized");

    let expected_note = project_dir.join("PWF-0002.md");
    assert_eq!(record.id, RecordId::Item(id));
    assert_eq!(record.title, "handle it");
    assert_eq!(record.status, WorkItemStatus::Active);
    assert_eq!(record.created, None);
    assert_eq!(record.completed, None);
    assert_eq!(record.section, None);
    assert_eq!(record.locator, path_str(&expected_note));
    assert_eq!(record.source, "");
    assert_eq!(record.body, "");
    assert_eq!(
        record.materialization,
        Materialization::MissingNote {
            expected: expected_note.display().to_string()
        }
    );
}

#[test]
fn index_entries_parse_open_done_and_raw_futuro_section() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("pwf.md"),
        concat!(
            "---\nid: pwf\ntitle: pwf\n---\n\n",
            "- [ ] [[PWF-0001]]\n",
            "- [x] [[PWF-0002]] \u{2705} 2026-07-02\n",
            "\n## Futuro\n",
            "- [ ] [[PWF-0003]]\n",
        ),
    )
    .unwrap();
    let store = ObsidianStore::new(raw_config_for_notes(&notes_dir));

    let project = ProjectName::try_new("pwf").unwrap();
    let entries = <ObsidianStore as AppDbStore<IndexEntry>>::list(&store, &project).unwrap();

    assert_eq!(
        entries,
        vec![
            IndexEntry {
                id: WorkItemId::try_new("PWF-0001").unwrap(),
                state: IndexEntryState::Open,
                section: String::new(),
            },
            IndexEntry {
                id: WorkItemId::try_new("PWF-0002").unwrap(),
                state: IndexEntryState::Done(Timestamp::new("2026-07-02")),
                section: String::new(),
            },
            IndexEntry {
                id: WorkItemId::try_new("PWF-0003").unwrap(),
                state: IndexEntryState::Open,
                section: "Futuro".to_string(),
            },
        ]
    );
}

#[test]
fn patch_status_done_flips_legacy_checkbox_with_date_stamp() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    let index_path = project_dir.join("pwf.md");
    std::fs::write(
        &index_path,
        "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0002|handle it]]\n",
    )
    .unwrap();
    let store = ObsidianStore::new(raw_config_for_notes(&notes_dir));

    let project = ProjectName::try_new("pwf").unwrap();
    let id = WorkItemId::try_new("PWF-0002").unwrap();
    let patch = ItemPatch {
        status: Some(WorkItemStatus::Done),
        completed: Some(Some(Timestamp::new("2026-07-15"))),
        ..Default::default()
    };

    <ObsidianStore as AppDbStore<PendingWorkItem>>::update(&store, &project, &id, patch).unwrap();

    let index = std::fs::read_to_string(&index_path).unwrap();
    assert!(
        index.contains("- [x] [[PWF-0002|handle it]] \u{2705} 2026-07-15"),
        "checkbox not flipped/stamped; got:\n{index}"
    );
}

#[test]
fn insert_allocates_next_id_without_index_write() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    let index_path = project_dir.join("pwf.md");
    let index_before = "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0007]]\n";
    std::fs::write(&index_path, index_before).unwrap();
    write_note(
        &project_dir.join("PWF-0007.md"),
        "existing",
        "2026-07-01",
        None,
        None,
        None,
        "already here",
    );
    let store = ObsidianStore::new(raw_config_for_notes(&notes_dir));

    let project = ProjectName::try_new("pwf").unwrap();
    let record = <ObsidianStore as AppDbStore<PendingWorkItem>>::insert(
        &store,
        &project,
        NewItem {
            prompt: "wire up the new thing".to_string(),
            title: None,
            created: Timestamp::new("2026-07-15"),
            section: None,
            prereq: None,
            effort: None,
            tags: None,
        },
    )
    .unwrap();

    assert_eq!(
        record.id,
        RecordId::Item(WorkItemId::try_new("PWF-0008").unwrap())
    );
    assert_eq!(record.status, WorkItemStatus::Active);
    assert!(record.source.contains("id: PWF-0008"));
    assert_eq!(record.locator, path_str(&project_dir.join("PWF-0008.md")));
    assert!(project_dir.join("PWF-0008.md").exists());
    assert_eq!(std::fs::read_to_string(&index_path).unwrap(), index_before);
}

/// Verifies that open index links contribute placement without owning list membership.
#[test]
fn generic_list_records_carry_open_placement_without_hiding_unlinked_notes() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    let index_path = project_dir.join("pwf.md");
    std::fs::write(
        &index_path,
        concat!(
            "---\nid: pwf\ntitle: pwf\n---\n\n",
            "## Futuro\n",
            "- [ ] [[PWF-0001|linked]]\n",
        ),
    )
    .unwrap();
    write_note(
        &project_dir.join("PWF-0001.md"),
        "linked",
        "2026-07-01",
        None,
        None,
        None,
        "body",
    );
    write_note(
        &project_dir.join("PWF-0002.md"),
        "unlinked",
        "2026-07-01",
        None,
        None,
        None,
        "body",
    );
    let store = ObsidianStore::new(raw_config_for_notes(&notes_dir));

    let project = ProjectName::try_new("pwf").unwrap();
    let records = <ObsidianStore as AppDbStore<PendingWorkItem>>::list(&store, &project).unwrap();

    assert_eq!(records.len(), 2);
    let record = records
        .iter()
        .find(|record| {
            record
                .id
                .as_item()
                .is_some_and(|id| id.as_ref() == "PWF-0001")
        })
        .expect("linked record must be listed");
    assert_eq!(
        record.id,
        RecordId::Item(WorkItemId::try_new("PWF-0001").unwrap())
    );
    assert_eq!(
        record.placement,
        Some(IndexPlacement {
            index_path: path_str(&index_path),
            line: 7,
        })
    );
    assert_eq!(record.section.as_deref(), Some("Futuro"));
    let unlinked = records
        .iter()
        .find(|record| {
            record
                .id
                .as_item()
                .is_some_and(|id| id.as_ref() == "PWF-0002")
        })
        .expect("unlinked record must be listed");
    assert!(unlinked.placement.is_none());
    assert_eq!(unlinked.section, None);
}

#[test]
fn list_pending_items_returns_note_history_and_index_only_records() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    let index_path = project_dir.join("pwf.md");
    std::fs::write(
        &index_path,
        concat!(
            "---\nid: pwf\ntitle: pwf\n---\n\n",
            "- [ ] [[PWF-0001|linked active]]\n",
            "## Human\n",
            "- [x] [[PWF-0002|linked done]] ✅ 2026-07-02\n",
            "- [x] [[PWF-0006|missing done]] ✅ 2026-07-06\n",
            "- [ ] `legacy task` :: run the legacy prompt\n",
        ),
    )
    .unwrap();
    for (id, status, completed) in [
        ("PWF-0001", "active", None),
        ("PWF-0002", "done", Some("2026-07-02")),
        ("PWF-0003", "cancelled", Some("2026-07-03")),
        ("PWF-0004", "done", Some("2026-07-04")),
        ("PWF-0005", "active", None),
    ] {
        write_status_note(
            &project_dir.join(format!("{id}.md")),
            &format!("title {id}"),
            status,
            completed,
            None,
        );
    }
    let store = ObsidianStore::new(raw_config_for_notes(&notes_dir));
    let project = ProjectName::try_new("pwf").unwrap();

    let records = <ObsidianStore as AppDbStore<PendingWorkItem>>::list(&store, &project).unwrap();
    let mut ids: Vec<String> = records
        .iter()
        .filter_map(|record| record.id.as_item().map(ToString::to_string))
        .collect();
    ids.sort();

    assert_eq!(
        ids,
        [
            "PWF-0001", "PWF-0002", "PWF-0003", "PWF-0004", "PWF-0005", "PWF-0006",
        ]
    );
    let record = |id: &str| {
        records
            .iter()
            .find(|record| record.id.as_item().is_some_and(|item| item.as_ref() == id))
            .unwrap_or_else(|| panic!("record {id} must be listed"))
    };
    assert_eq!(
        record("PWF-0001").placement,
        Some(IndexPlacement {
            index_path: path_str(&index_path),
            line: 6,
        })
    );
    assert_eq!(record("PWF-0002").status, WorkItemStatus::Done);
    assert_eq!(record("PWF-0002").section.as_deref(), Some("Human"));
    assert!(record("PWF-0002").placement.is_none());
    assert_eq!(record("PWF-0003").status, WorkItemStatus::Cancelled);
    assert_eq!(record("PWF-0004").status, WorkItemStatus::Done);
    assert!(record("PWF-0004").placement.is_none());
    assert_eq!(record("PWF-0005").status, WorkItemStatus::Active);
    assert!(record("PWF-0005").placement.is_none());
    let missing_done = record("PWF-0006");
    assert_eq!(missing_done.status, WorkItemStatus::Done);
    assert_matches!(
        &missing_done.materialization,
        Materialization::MissingNote { .. }
    );
    assert!(records.iter().any(|record| {
        record.id == RecordId::Inline(1)
            && record.title == "legacy task"
            && record.body == "run the legacy prompt"
    }));
}

#[test]
fn list_pending_items_returns_note_history_when_index_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    write_status_note(
        &project_dir.join("PWF-0001.md"),
        "evicted done task",
        "done",
        Some("2026-07-01"),
        None,
    );
    let store = ObsidianStore::new(raw_config_for_notes(&notes_dir));
    let project = ProjectName::try_new("pwf").unwrap();

    let records = <ObsidianStore as AppDbStore<PendingWorkItem>>::list(&store, &project).unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, WorkItemStatus::Done);
    assert!(records[0].placement.is_none());
}

#[test]
fn index_sections_list_raw_h2_labels_in_document_order() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("pwf.md"),
        concat!(
            "---\nid: pwf\ntitle: pwf\n---\n\n",
            "- [ ] [[PWF-0001|alias]]\n\n",
            "## Human\n- [ ] [[PWF-0002|h]]\n\n",
            "## Futuro\n\n",
            "## Low-prio\n\n",
            "### Notes\n- [[PWF-NOTE-0001]]\n",
        ),
    )
    .unwrap();
    let store = ObsidianStore::new(raw_config_for_notes(&notes_dir));
    let project = ProjectName::try_new("pwf").unwrap();

    let sections = <ObsidianStore as AppDbStore<IndexSection>>::list(&store, &project).unwrap();

    // RAW labels in document order; H3 regions (### Notes) are not sections.
    assert_eq!(
        sections,
        vec![
            IndexSection {
                label: "Human".to_string()
            },
            IndexSection {
                label: "Futuro".to_string()
            },
            IndexSection {
                label: "Low-prio".to_string()
            },
        ]
    );
}

#[test]
fn index_sections_list_empty_when_index_missing() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    std::fs::create_dir_all(notes_dir.join("pwf")).unwrap();
    let store = ObsidianStore::new(raw_config_for_notes(&notes_dir));
    let project = ProjectName::try_new("pwf").unwrap();

    let sections = <ObsidianStore as AppDbStore<IndexSection>>::list(&store, &project).unwrap();

    assert!(sections.is_empty());
}

#[test]
fn index_section_get_finds_exact_raw_label() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("pwf.md"),
        "---\nid: pwf\ntitle: pwf\n---\n\n## Human\n",
    )
    .unwrap();
    let store = ObsidianStore::new(raw_config_for_notes(&notes_dir));
    let project = ProjectName::try_new("pwf").unwrap();

    let human =
        <ObsidianStore as AppDbStore<IndexSection>>::get(&store, &project, &"Human".to_string())
            .unwrap();
    let missing =
        <ObsidianStore as AppDbStore<IndexSection>>::get(&store, &project, &"Future".to_string())
            .unwrap();

    assert_eq!(
        human,
        Some(IndexSection {
            label: "Human".to_string()
        })
    );
    assert_eq!(
        missing, None,
        "get is exact-raw-label; no alias policy here"
    );
}

#[test]
fn index_section_insert_and_delete_are_unsupported() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    std::fs::create_dir_all(notes_dir.join("pwf")).unwrap();
    let store = ObsidianStore::new(raw_config_for_notes(&notes_dir));
    let project = ProjectName::try_new("pwf").unwrap();
    let section = IndexSection {
        label: "Human".to_string(),
    };

    assert_matches!(
        <ObsidianStore as AppDbStore<IndexSection>>::insert(&store, &project, section),
        Err(ObsidianStoreError::IndexSectionWriteUnsupported { op: "insert" })
    );
    assert_matches!(
        <ObsidianStore as AppDbStore<IndexSection>>::delete(&store, &project, &"Human".to_string()),
        Err(ObsidianStoreError::IndexSectionWriteUnsupported { op: "delete" })
    );
}

/// Verifies that section update renames an H2 label in place.
#[test]
fn index_section_update_renames_header_in_place() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("pwf.md"),
        "---\nid: pwf\ntitle: pwf\n---\n\n## Futuro\n\n- [ ] [[PWF-0001]]\n",
    )
    .unwrap();
    let store = ObsidianStore::new(raw_config_for_notes(&notes_dir));
    let project = ProjectName::try_new("pwf").unwrap();

    <ObsidianStore as AppDbStore<IndexSection>>::update(
        &store,
        &project,
        &"Futuro".to_string(),
        IndexSection {
            label: "Future".to_string(),
        },
    )
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(project_dir.join("pwf.md")).unwrap(),
        "---\nid: pwf\ntitle: pwf\n---\n\n## Future\n\n- [ ] [[PWF-0001]]\n"
    );
}

/// Captures one add scenario and its expected byte-exact index.
struct AddParityScenario {
    name: &'static str,
    initial_index: Option<&'static str>,
    section: Option<&'static str>,
    expected_index: &'static str,
}

const PARITY_IDENTITY: &str = "---\nid: pwf\ntitle: pwf\n---\n\n";

fn add_parity_scenarios() -> Vec<AddParityScenario> {
    vec![
        AddParityScenario {
            name: "general section with existing anchor",
            initial_index: Some("---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0001|tray gui]]\n"),
            section: None,
            expected_index: "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0002]]\n- [ ] [[PWF-0001|tray gui]]\n",
        },
        AddParityScenario {
            name: "human section created before existing future",
            initial_index: Some(
                "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0001|tray gui]]\n\n## Future\n\n- [ ] [[PWF-0009|later]]\n",
            ),
            section: Some("Human"),
            expected_index: "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0001|tray gui]]\n\n## Human\n\n- [ ] [[PWF-0002]]\n## Future\n\n- [ ] [[PWF-0009|later]]\n",
        },
        AddParityScenario {
            name: "existing empty human header is not re-created",
            initial_index: Some(
                "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0001|tray gui]]\n\n## Human\n",
            ),
            section: Some("Human"),
            expected_index: "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0001|tray gui]]\n\n## Human\n- [ ] [[PWF-0002]]\n",
        },
        AddParityScenario {
            name: "future lands under legacy futuro alias header",
            initial_index: Some(
                "---\nid: pwf\ntitle: pwf\n---\n\n## Futuro\n\n- [ ] [[PWF-0009|later]]\n",
            ),
            section: Some("Future"),
            expected_index: "---\nid: pwf\ntitle: pwf\n---\n\n## Futuro\n- [ ] [[PWF-0002]]\n\n- [ ] [[PWF-0009|later]]\n",
        },
        AddParityScenario {
            name: "low-prio section created at end",
            initial_index: Some("---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0001|tray gui]]\n"),
            section: Some("Low-prio"),
            expected_index: "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0001|tray gui]]\n\n## Low-prio\n\n- [ ] [[PWF-0002]]\n",
        },
        AddParityScenario {
            name: "fresh vault creates identity template",
            initial_index: None,
            section: None,
            expected_index: "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0002]]\n",
        },
        AddParityScenario {
            name: "fresh vault with section creates template and section",
            initial_index: None,
            section: Some("Human"),
            expected_index: "---\nid: pwf\ntitle: pwf\n---\n\n## Human\n\n- [ ] [[PWF-0002]]\n",
        },
        AddParityScenario {
            name: "general add stays above trailing notes block",
            initial_index: Some(
                "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0001|tray gui]]\n\n### Notes\n- [[PWF-NOTE-0001]]\n",
            ),
            section: None,
            expected_index: "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0002]]\n- [ ] [[PWF-0001|tray gui]]\n\n### Notes\n- [[PWF-NOTE-0001]]\n",
        },
    ]
}

fn stage_add_parity_vault(
    initial_index: Option<&str>,
) -> (tempfile::TempDir, ObsidianStore, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    write_note(
        &project_dir.join("PWF-0001.md"),
        "tray gui",
        "2026-01-01",
        None,
        None,
        None,
        "add toggle",
    );
    if let Some(index) = initial_index {
        std::fs::write(project_dir.join("pwf.md"), index).unwrap();
    }
    let store = ObsidianStore::new(raw_config_for_notes(&notes_dir));
    (temp, store, project_dir)
}

/// Verifies byte-exact section placement, H3 safety, link format, and fresh-index creation.
#[test]
fn generic_insert_plus_upsert_writes_legacy_add_index_bytes() {
    for scenario in add_parity_scenarios() {
        let (_guard, store, project_dir) = stage_add_parity_vault(scenario.initial_index);
        let project = ProjectName::try_new("pwf").unwrap();

        let record = <ObsidianStore as AppDbStore<PendingWorkItem>>::insert(
            &store,
            &project,
            NewItem {
                prompt: "do the thing".to_string(),
                title: Some("Ship It".to_string()),
                created: Timestamp::new("2026-07-07"),
                section: scenario.section.map(str::to_string),
                prereq: None,
                effort: None,
                tags: None,
            },
        )
        .unwrap();
        let id = record
            .id
            .as_item()
            .expect("inserted record has an id")
            .clone();
        assert_eq!(
            id.as_ref(),
            "PWF-0002",
            "allocated id for `{}`",
            scenario.name
        );
        <ObsidianStore as AppDbStore<IndexEntry>>::insert(
            &store,
            &project,
            IndexEntry {
                id,
                state: IndexEntryState::Open,
                section: scenario.section.unwrap_or("").to_string(),
            },
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(project_dir.join("pwf.md")).unwrap(),
            scenario.expected_index,
            "index bytes for scenario `{}`",
            scenario.name
        );
        assert!(
            project_dir.join("PWF-0002.md").exists(),
            "note file for scenario `{}`",
            scenario.name
        );
    }
}

#[test]
fn upsert_creates_missing_index_from_identity_template() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    std::fs::create_dir_all(notes_dir.join("pwf")).unwrap();
    let store = ObsidianStore::new(raw_config_for_notes(&notes_dir));
    let project = ProjectName::try_new("pwf").unwrap();

    <ObsidianStore as AppDbStore<IndexEntry>>::insert(
        &store,
        &project,
        IndexEntry {
            id: WorkItemId::try_new("PWF-0001").unwrap(),
            state: IndexEntryState::Open,
            section: String::new(),
        },
    )
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(notes_dir.join("pwf/pwf.md")).unwrap(),
        "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0001]]\n"
    );
}

#[test]
fn delete_index_entry_errors_when_no_link_matches() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("pwf.md"), PARITY_IDENTITY).unwrap();
    let store = ObsidianStore::new(raw_config_for_notes(&notes_dir));
    let project = ProjectName::try_new("pwf").unwrap();

    let error = <ObsidianStore as AppDbStore<IndexEntry>>::delete(
        &store,
        &project,
        &WorkItemId::try_new("PWF-0002").unwrap(),
    )
    .unwrap_err();

    assert_matches!(
        error,
        ObsidianStoreError::IndexLinkNotFound { ref id } if id == "PWF-0002"
    );
    assert_eq!(error.to_string(), "Index link not found for PWF-0002.");
}
