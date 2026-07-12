use std::{assert_matches, fmt::Write as _, path::Path};

use pwf_application::{
    AddItemSpec, CancelItemSpec, ClosedItemAction, CompleteItemSpec, PendingWorkReadStore,
    PendingWorkResolveStore, PendingWorkWriteStore, ReopenedItem, ResolvePendingWorkOutput,
    StatusTransitionDiagnostics, UpdateItemSpec,
};
use pwf_core::config::from_json;
use pwf_domain::pending_work::{OpenItem, Tags};

use super::{ObsidianPendingWorkStore, ObsidianPendingWorkStoreError, fs::path_str};

#[test]
fn open_items_matches_file_parser_metadata_for_normal_human_and_future_tasks() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();

    std::fs::write(
        project_dir.join("pwf.md"),
        concat!(
            "# pwf\n",
            "- [ ] [[PWF-0001|normal alias]]\n",
            "\n",
            "## Human\n",
            "- [ ] [[PWF-0002|human alias]]\n",
            "\n",
            "## Future\n",
            "- [ ] [[PWF-0003|future alias]]\n",
        ),
    )
    .unwrap();

    write_note(
        &project_dir.join("PWF-0001.md"),
        "normal title",
        "2026-07-01",
        Some("\"[[CFG-0001]]\""),
        Some("2"),
        Some("[sqlite, godot]"),
        "ship the adapter",
    );
    write_note(
        &project_dir.join("PWF-0002.md"),
        "human title",
        "2026-07-02",
        None,
        None,
        None,
        "ask the human to verify",
    );
    write_note(
        &project_dir.join("PWF-0003.md"),
        "future title",
        "2026-07-03",
        None,
        Some("4"),
        None,
        "follow up later",
    );

    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let got = store.all_open_items().unwrap();

    assert_eq!(
        got,
        vec![
            expected_item(&ItemExpectation {
                id: "PWF-0001",
                session: "normal title",
                prompt: "ship the adapter",
                repo: "/repo/pwf",
                project_dir: &project_dir,
                line: 7,
                section: None,
                prereq: Some("\"[[CFG-0001]]\""),
                effort: Some("2"),
                tags: Some("[sqlite, godot]"),
                created: Some("2026-07-01"),
            }),
            expected_item(&ItemExpectation {
                id: "PWF-0002",
                session: "human title",
                prompt: "ask the human to verify",
                repo: "/repo/pwf",
                project_dir: &project_dir,
                line: 10,
                section: Some("Human"),
                prereq: None,
                effort: None,
                tags: None,
                created: Some("2026-07-02"),
            }),
            expected_item(&ItemExpectation {
                id: "PWF-0003",
                session: "future title",
                prompt: "follow up later",
                repo: "/repo/pwf",
                project_dir: &project_dir,
                line: 13,
                section: Some("Future"),
                prereq: None,
                effort: Some("4"),
                tags: None,
                created: Some("2026-07-03"),
            }),
        ]
    );
}

#[test]
fn open_items_rejects_project_index_without_identity_frontmatter() {
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
    let store = ObsidianPendingWorkStore::new(raw_config_for_notes(&notes_dir));

    let error = store.all_open_items().unwrap_err();

    assert_matches!(
        error,
        ObsidianPendingWorkStoreError::MissingFrontmatter { ref property, .. }
            if *property == "id/title"
    );
}

#[test]
fn open_items_retains_raw_tags_frontmatter() {
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
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    assert_eq!(
        store.all_open_items().unwrap()[0].tags.as_deref(),
        Some("[SQLite, malformed-but-displayable]")
    );
}

#[test]
fn open_items_use_yaml_decoded_title() {
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
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let item = store.all_open_items().unwrap().remove(0);

    assert_eq!(item.session, "adapter: preserve identity");
}

#[test]
fn resolve_item_uses_frontmatter_id_instead_of_filename() {
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
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let resolved = store.resolve_item("PWF-0001", false).unwrap();

    assert_eq!(
        resolved,
        ResolvePendingWorkOutput::NotePath(path_str(&project_dir.join("descriptive-name.md")))
    );
}

#[test]
fn resolve_item_rejects_duplicate_frontmatter_ids() {
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
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let error = store.resolve_item("PWF-0001", false).unwrap_err();

    assert_matches!(
        error,
        ObsidianPendingWorkStoreError::DuplicateTaskId { ref id, .. } if id == "PWF-0001"
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

struct ItemExpectation<'a> {
    id: &'a str,
    session: &'a str,
    prompt: &'a str,
    repo: &'a str,
    project_dir: &'a Path,
    line: usize,
    section: Option<&'a str>,
    prereq: Option<&'a str>,
    effort: Option<&'a str>,
    tags: Option<&'a str>,
    created: Option<&'a str>,
}

fn expected_item(expectation: &ItemExpectation<'_>) -> OpenItem {
    OpenItem {
        id: expectation.id.to_string(),
        project: "pwf".to_string(),
        session: expectation.session.to_string(),
        prompt: expectation.prompt.to_string(),
        repo: Some(expectation.repo.to_string()),
        note: path_str(&expectation.project_dir.join("pwf.md")),
        item_file: Some(path_str(
            &expectation
                .project_dir
                .join(format!("{}.md", expectation.id)),
        )),
        line: expectation.line,
        format: "file".to_string(),
        launchable: true,
        needs_prompt: false,
        issues: vec![],
        section: expectation.section.map(str::to_string),
        prereq: expectation.prereq.map(str::to_string),
        effort: expectation.effort.map(str::to_string),
        tags: expectation.tags.map(str::to_string),
        created: expectation.created.map(str::to_string),
    }
}

#[test]
fn add_item_creates_note_and_links_index() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let config = config_for_notes(&notes_dir);
    let store = ObsidianPendingWorkStore::new(config);

    let added = store
        .add_item(AddItemSpec {
            project_name: "pwf".to_string(),
            prompt: "Ship the adapter /d tests pass".to_string(),
            title: Some("Ship Adapter".to_string()),
            created: "2026-07-07".to_string(),
            section: Some("Human".to_string()),
            prereq: Some("[[PWF-0001]]".to_string()),
            effort: Some(2),
            tags: None,
        })
        .unwrap();

    assert_eq!(added.id, "PWF-0001");
    assert_eq!(added.project, "pwf");
    assert_eq!(added.title, "ship adapter");
    assert_eq!(added.created_section, Some("Human".to_string()));
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
fn add_item_writes_canonical_tags_and_omits_absent_tags() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));
    let tags = Tags::parse_values(&["SQLite,csharp-export".to_string()]).unwrap();
    let tagged = store
        .add_item(AddItemSpec {
            project_name: "pwf".to_string(),
            prompt: "tagged task".to_string(),
            title: None,
            created: "2026-07-07".to_string(),
            section: None,
            prereq: None,
            effort: None,
            tags: Some(tags),
        })
        .unwrap();
    let note = std::fs::read_to_string(tagged.note_path).unwrap();
    assert!(note.contains("tags: [sqlite, csharp_export]\n"), "{note}");

    let untagged = store
        .add_item(AddItemSpec {
            project_name: "pwf".to_string(),
            prompt: "untagged task".to_string(),
            title: None,
            created: "2026-07-07".to_string(),
            section: None,
            prereq: None,
            effort: None,
            tags: None,
        })
        .unwrap();
    let note = std::fs::read_to_string(untagged.note_path).unwrap();
    assert!(!note.contains("tags:"), "{note}");
}

#[test]
fn add_item_rejects_unreadable_existing_index_before_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(project_dir.join("pwf.md")).unwrap();
    let config = config_for_notes(&notes_dir);
    let store = ObsidianPendingWorkStore::new(config);

    let err = store
        .add_item(AddItemSpec {
            project_name: "pwf".to_string(),
            prompt: "Ship the adapter /d tests pass".to_string(),
            title: Some("Ship Adapter".to_string()),
            created: "2026-07-07".to_string(),
            section: Some("Human".to_string()),
            prereq: None,
            effort: None,
            tags: None,
        })
        .unwrap_err();

    assert!(err.to_string().starts_with("Cannot read index: "));
    assert_eq!(err.created_section_diagnostic(), None);
}

#[test]
fn add_item_rejects_mismatched_project_index_identity() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("pwf.md"),
        "---\nid: rst\ntitle: rust-learn\n---\n\n# wrong\n",
    )
    .unwrap();
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let error = store
        .add_item(AddItemSpec {
            project_name: "pwf".to_string(),
            prompt: "task".to_string(),
            title: None,
            created: "2026-07-12".to_string(),
            section: None,
            prereq: None,
            effort: None,
            tags: None,
        })
        .unwrap_err();

    assert_matches!(
        error,
        ObsidianPendingWorkStoreError::ProjectIndexIdentityMismatch { .. }
    );
}

#[test]
fn add_item_allocates_after_greatest_frontmatter_id() {
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
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let added = store
        .add_item(AddItemSpec {
            project_name: "pwf".to_string(),
            prompt: "next task".to_string(),
            title: None,
            created: "2026-07-12".to_string(),
            section: None,
            prereq: None,
            effort: None,
            tags: None,
        })
        .unwrap();

    assert_eq!(added.id, "PWF-0010");
}

#[test]
fn update_item_rewrites_body_prereq_commits_effort_and_report() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("pwf.md"),
        "- [ ] [[PWF-0001]]\n- [ ] [[PWF-0002]]\n",
    )
    .unwrap();
    write_note(
        &project_dir.join("PWF-0001.md"),
        "prereq",
        "2026-07-01",
        None,
        None,
        None,
        "already exists",
    );
    write_note(
        &project_dir.join("PWF-0002.md"),
        "old title",
        "2026-07-02",
        None,
        None,
        None,
        "old body",
    );
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let updated = store
        .update_item(UpdateItemSpec {
            id: "pwf-2".to_string(),
            prompt: Some("new prompt".to_string()),
            title: Some("Renamed".to_string()),
            append: Some("extra goal /c more context".to_string()),
            prereq: vec!["PWF-0001".to_string()],
            clear_prereq: false,
            commits: Some("a..b".to_string()),
            append_report: Some("## Result\n\nDone.".to_string()),
            effort: Some(3),
            tags: None,
            tags_clear: false,
        })
        .unwrap();

    assert_eq!(
        updated,
        pwf_domain::pending_work::UpdatedItem::OpenItemEdit {
            id: "PWF-0002".to_string(),
            project: "pwf".to_string(),
            title: "renamed".to_string(),
        }
    );
    let note = std::fs::read_to_string(project_dir.join("PWF-0002.md")).unwrap();
    assert!(note.contains("title: renamed"), "{note}");
    assert!(note.contains("prereq: \"[[PWF-0001]]\""), "{note}");
    assert!(note.contains("commits: \"a..b\""), "{note}");
    assert!(note.contains("effort: 3"), "{note}");
    assert!(
        note.contains("## Goals\n- new prompt\n- extra goal"),
        "{note}"
    );
    assert!(note.contains("## Context\n- more context"), "{note}");
    assert!(
        note.contains("### Report\n\n## Result\n\nDone.\n"),
        "{note}"
    );
}

fn tag_update(id: &str, tags: Option<Tags>, tags_clear: bool) -> UpdateItemSpec {
    UpdateItemSpec {
        id: id.to_string(),
        prompt: None,
        title: None,
        append: None,
        prereq: Vec::new(),
        clear_prereq: false,
        commits: None,
        append_report: None,
        effort: None,
        tags,
        tags_clear,
    }
}

#[test]
fn update_item_appends_deduplicated_tags() {
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item_with_tags("[sqlite, godot]");
    let tags = Tags::parse_values(&["godot,csharp-export".to_string()]).unwrap();

    store
        .update_item(tag_update("PWF-0001", Some(tags), false))
        .unwrap();

    let note = std::fs::read_to_string(item_path).unwrap();
    assert!(
        note.contains("tags: [sqlite, godot, csharp_export]\n"),
        "{note}"
    );
    assert_eq!(note.matches("tags:").count(), 1);
}

#[test]
fn update_item_append_adds_frontmatter_tags_without_rewriting_body_tags_line() {
    let body = "tags: body-only value\nkeep this body byte-identical";
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item(None, body);
    let tags = Tags::parse_values(&["SQLite".to_string()]).unwrap();

    store
        .update_item(tag_update("PWF-0001", Some(tags), false))
        .unwrap();

    let note = std::fs::read_to_string(item_path).unwrap();
    assert_eq!(
        note,
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
fn update_item_clear_preserves_body_tags_line_when_frontmatter_has_no_tags() {
    let body = "tags: body-only value\nkeep this body byte-identical";
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item(None, body);
    let before = std::fs::read_to_string(&item_path).unwrap();

    store
        .update_item(tag_update("PWF-0001", None, true))
        .unwrap();

    assert_eq!(std::fs::read_to_string(item_path).unwrap(), before);
}

#[test]
fn update_item_append_supports_bom_frontmatter_format() {
    let before = formatted_tag_note(true, "\n", None);
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item_from_note(&before);
    let tags = Tags::parse_values(&["SQLite".to_string()]).unwrap();

    store
        .update_item(tag_update("PWF-0001", Some(tags), false))
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(item_path).unwrap(),
        formatted_tag_note(true, "\n", Some("[sqlite]"))
    );
}

#[test]
fn update_item_clear_supports_bom_frontmatter_format() {
    let before = formatted_tag_note(true, "\n", Some("[godot]"));
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item_from_note(&before);

    store
        .update_item(tag_update("PWF-0001", None, true))
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(item_path).unwrap(),
        formatted_tag_note(true, "\n", None)
    );
}

#[test]
fn update_item_append_supports_crlf_frontmatter_format() {
    let before = formatted_tag_note(false, "\r\n", None);
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item_from_note(&before);
    let tags = Tags::parse_values(&["SQLite".to_string()]).unwrap();

    store
        .update_item(tag_update("PWF-0001", Some(tags), false))
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(item_path).unwrap(),
        formatted_tag_note(false, "\r\n", Some("[sqlite]"))
    );
}

#[test]
fn update_item_clear_supports_crlf_frontmatter_format() {
    let before = formatted_tag_note(false, "\r\n", Some("[godot]"));
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item_from_note(&before);

    store
        .update_item(tag_update("PWF-0001", None, true))
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(item_path).unwrap(),
        formatted_tag_note(false, "\r\n", None)
    );
}

#[test]
fn update_item_clear_removes_tags_and_clear_plus_tags_replaces() {
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item_with_tags("[godot, setup]");
    store
        .update_item(tag_update("PWF-0001", None, true))
        .unwrap();
    let note = std::fs::read_to_string(&item_path).unwrap();
    assert!(!note.contains("tags:"), "{note}");

    std::fs::write(
        &item_path,
        "---\nid: PWF-0001\nstatus: active\ntitle: tagged\nproject: pwf\ncreated: 2026-07-01\ntags: [godot, setup]\n---\n\nbody\n",
    )
    .unwrap();
    let replacement = Tags::parse_values(&["SQLite".to_string()]).unwrap();
    store
        .update_item(tag_update("PWF-0001", Some(replacement), true))
        .unwrap();
    let note = std::fs::read_to_string(item_path).unwrap();
    assert!(note.contains("tags: [sqlite]\n"), "{note}");
    assert!(!note.contains("godot"), "{note}");
}

#[test]
fn update_item_clear_and_replace_do_not_parse_corrupt_existing_tags() {
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item_with_tags("sqlite, godot");
    store
        .update_item(tag_update("PWF-0001", None, true))
        .unwrap();
    let note = std::fs::read_to_string(&item_path).unwrap();
    assert!(!note.contains("tags:"), "{note}");

    std::fs::write(
        &item_path,
        "---\nid: PWF-0001\nstatus: active\ntitle: tagged\nproject: pwf\ncreated: 2026-07-01\ntags: still-corrupt\n---\n\nbody\n",
    )
    .unwrap();
    let replacement = Tags::parse_values(&["SQLite".to_string()]).unwrap();
    store
        .update_item(tag_update("PWF-0001", Some(replacement), true))
        .unwrap();
    let note = std::fs::read_to_string(item_path).unwrap();
    assert!(note.contains("tags: [sqlite]\n"), "{note}");
    assert!(!note.contains("still-corrupt"), "{note}");
}

#[test]
fn update_item_rejects_corrupt_existing_tags_before_writing() {
    let StagedOpenItem {
        _temp,
        store,
        item_path,
    } = staged_open_item_with_tags("sqlite, godot");
    let before = std::fs::read_to_string(&item_path).unwrap();
    let tags = Tags::parse_values(&["sqlite".to_string()]).unwrap();

    let error = store
        .update_item(tag_update("PWF-0001", Some(tags), false))
        .unwrap_err();

    match error {
        ObsidianPendingWorkStoreError::InvalidTagsFrontmatter { id, raw } => {
            assert_eq!(id, "PWF-0001");
            assert_eq!(raw, "sqlite, godot");
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert_eq!(std::fs::read_to_string(item_path).unwrap(), before);
}

#[test]
fn update_item_rejects_empty_tags_frontmatter_before_writing() {
    for raw in ["", "   "] {
        let StagedOpenItem {
            _temp,
            store,
            item_path,
        } = staged_open_item_with_tags(raw);
        let before = std::fs::read_to_string(&item_path).unwrap();
        let tags = Tags::parse_values(&["sqlite".to_string()]).unwrap();

        let error = store
            .update_item(tag_update("PWF-0001", Some(tags), false))
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "item PWF-0001 has invalid tags frontmatter: \"\"."
        );
        assert_eq!(std::fs::read_to_string(item_path).unwrap(), before);
    }
}

#[test]
fn remove_item_deletes_note_and_unlinks_index() {
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
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let removed = store.remove_item("PWF-0001").unwrap();

    assert_eq!(removed.id, "PWF-0001");
    assert_eq!(removed.project, "pwf");
    assert_eq!(removed.title, "stale task");
    assert!(!project_dir.join("PWF-0001.md").exists());
    assert_eq!(
        std::fs::read_to_string(project_dir.join("pwf.md")).unwrap(),
        "---\nid: pwf\ntitle: pwf\n---\n"
    );
}

#[test]
fn resolve_item_returns_open_note_path_from_active_index() {
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
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let got = store.resolve_item("PWF-0001", false).unwrap();

    assert_eq!(
        got,
        ResolvePendingWorkOutput::NotePath(path_str(&project_dir.join("PWF-0001.md")))
    );
}

#[test]
fn resolve_item_show_returns_open_note_markdown_with_created_key() {
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
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let got = store.resolve_item("PWF-0001", true).unwrap();

    assert_eq!(
        got,
        ResolvePendingWorkOutput::NoteMarkdown(
            "---\nid: PWF-0001\nstatus: active\ntitle: active task\nproject: pwf\ncreated: 2026-07-01\n---\n\n## Goals\n- body\n"
                .to_string(),
        )
    );
}

#[test]
fn resolve_item_show_finds_closed_note_still_in_project_dir() {
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
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let got = store.resolve_item("PWF-0003", true).unwrap();

    assert_eq!(
        got,
        ResolvePendingWorkOutput::NoteMarkdown(
            "---\nid: PWF-0003\nstatus: done\ntitle: done task\nproject: pwf\ncreated: 2026-07-01\ncompleted: 2026-07-07\n---\n\nbody\n"
                .to_string(),
        )
    );
}

#[test]
fn resolve_item_show_finds_closed_note_still_in_project_dir_with_shorthand_id() {
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
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let got = store.resolve_item("pwf3", true).unwrap();

    assert_eq!(
        got,
        ResolvePendingWorkOutput::NoteMarkdown(
            "---\nid: PWF-0003\nstatus: done\ntitle: done task\nproject: pwf\ncreated: 2026-07-01\ncompleted: 2026-07-07\n---\n\nbody\n"
                .to_string(),
        )
    );
}

#[test]
fn resolve_item_not_found_preserves_raw_requested_id() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    std::fs::create_dir_all(notes_dir.join("pwf")).unwrap();
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let err = store.resolve_item("pwf-9999", false).unwrap_err();

    assert!(matches!(
        err,
        crate::obsidian::ObsidianPendingWorkStoreError::ItemNotFound { id } if id == "pwf-9999"
    ));
}

#[test]
fn find_pending_item_ambiguous_error_preserves_raw_requested_id() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let alpha_dir = notes_dir.join("alpha");
    let beta_dir = notes_dir.join("beta");
    std::fs::create_dir_all(&alpha_dir).unwrap();
    std::fs::create_dir_all(&beta_dir).unwrap();
    std::fs::write(alpha_dir.join("alpha.md"), "- [ ] [[PWF-0001]]\n").unwrap();
    std::fs::write(beta_dir.join("beta.md"), "- [ ] [[PWF-0001]]\n").unwrap();
    std::fs::write(
        alpha_dir.join("PWF-0001.md"),
        "---\nid: PWF-0001\nstatus: active\ntitle: alpha task\nproject: alpha\ncreated: 2026-07-01\n---\n\nbody\n",
    )
    .unwrap();
    std::fs::write(
        beta_dir.join("PWF-0001.md"),
        "---\nid: PWF-0001\nstatus: active\ntitle: beta task\nproject: beta\ncreated: 2026-07-02\n---\n\nbody\n",
    )
    .unwrap();
    let store = ObsidianPendingWorkStore::new(config_for_projects(
        &notes_dir,
        &[("alpha", "PWF"), ("beta", "PWF")],
    ));

    let err = store.find_pending_item("pwf-0001").unwrap_err();

    assert!(matches!(
        err,
        crate::obsidian::ObsidianPendingWorkStoreError::AmbiguousId { id } if id == "pwf-0001"
    ));
}

#[test]
fn complete_item_marks_active_file_item_done_with_report_and_commits() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("pwf.md"), "- [ ] [[PWF-0001]]\n").unwrap();
    write_note(
        &project_dir.join("PWF-0001.md"),
        "ship it",
        "2026-07-01",
        None,
        None,
        None,
        "body",
    );
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let closed = store
        .complete_item(CompleteItemSpec {
            id: "pwf-1".to_string(),
            completed: "2026-07-07".to_string(),
            report: Some(" did one thing\nand another ".to_string()),
            commits: Some("a..b".to_string()),
        })
        .unwrap();

    assert_eq!(closed.id, "PWF-0001");
    assert_eq!(closed.project, "pwf");
    assert_eq!(closed.title, "ship it");
    assert_eq!(closed.action, ClosedItemAction::Done);
    assert_eq!(closed.diagnostics, StatusTransitionDiagnostics::none());
    let note = std::fs::read_to_string(project_dir.join("PWF-0001.md")).unwrap();
    assert!(note.contains("status: done"), "{note}");
    assert!(note.contains("completed: 2026-07-07"), "{note}");
    assert!(note.contains("commits: \"a..b\""), "{note}");
    assert!(
        note.contains("### Report\n\ndid one thing and another\n"),
        "{note}"
    );
    assert_eq!(
        std::fs::read_to_string(project_dir.join("pwf.md")).unwrap(),
        "---\nid: pwf\ntitle: pwf\n---\n\n- [x] [[PWF-0001]] ✅ 2026-07-07\n"
    );
}

#[test]
fn cancel_item_marks_active_file_item_cancelled_with_required_report() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("pwf.md"), "- [ ] [[PWF-0001]]\n").unwrap();
    write_note(
        &project_dir.join("PWF-0001.md"),
        "stop it",
        "2026-07-01",
        None,
        None,
        None,
        "body",
    );
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let closed = store
        .cancel_item(CancelItemSpec {
            id: "PWF-0001".to_string(),
            completed: "2026-07-07".to_string(),
            report: "blocked upstream".to_string(),
            commits: None,
        })
        .unwrap();

    assert_eq!(closed.action, ClosedItemAction::Cancelled);
    let note = std::fs::read_to_string(project_dir.join("PWF-0001.md")).unwrap();
    assert!(note.contains("status: cancelled"), "{note}");
    assert!(note.contains("completed: 2026-07-07"), "{note}");
    assert!(note.contains("### Report\n\nblocked upstream\n"), "{note}");
    assert_eq!(
        store
            .cancel_item(CancelItemSpec {
                id: "PWF-0001".to_string(),
                completed: "2026-07-07".to_string(),
                report: " \n ".to_string(),
                commits: None,
            })
            .unwrap_err()
            .to_string(),
        "--report cannot be empty."
    );
}

#[test]
fn complete_item_rotates_done_queue_and_keeps_evicted_notes() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    let mut index_lines = Vec::new();
    for n in 1..=6 {
        let id = format!("PWF-{n:04}");
        index_lines.push(format!("- [x] [[{id}]] ✅ 2026-01-{n:02}"));
        write_status_note(
            &project_dir.join(format!("{id}.md")),
            &format!("done {n}"),
            "done",
            Some(&format!("2026-01-{n:02}")),
            None,
        );
    }
    index_lines.push("- [ ] [[PWF-0007]]".to_string());
    std::fs::write(
        project_dir.join("pwf.md"),
        format!("{}\n", index_lines.join("\n")),
    )
    .unwrap();
    write_note(
        &project_dir.join("PWF-0007.md"),
        "new done",
        "2026-07-01",
        None,
        None,
        None,
        "body",
    );
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let closed = store
        .complete_item(CompleteItemSpec {
            id: "PWF-0007".to_string(),
            completed: "2026-07-07".to_string(),
            report: None,
            commits: None,
        })
        .unwrap();

    assert_eq!(closed.diagnostics.evicted_ids, vec!["PWF-0001"]);
    assert!(project_dir.join("PWF-0001.md").exists());
    assert!(!project_dir.join("_archive").exists());
    let index = std::fs::read_to_string(project_dir.join("pwf.md")).unwrap();
    assert!(!index.contains("PWF-0001"), "{index}");
    assert!(
        index.contains("- [x] [[PWF-0007]] ✅ 2026-07-07"),
        "{index}"
    );
    assert_eq!(index.matches("- [x]").count(), 6);
}

#[test]
fn reopen_item_flips_done_and_cancelled_notes_back_to_active() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("pwf.md"),
        "- [x] [[PWF-0001]] ✅ 2026-07-07\n- [x] [[PWF-0002]] ✅ 2026-07-07\n",
    )
    .unwrap();
    write_status_note(
        &project_dir.join("PWF-0001.md"),
        "done item",
        "done",
        Some("2026-07-07"),
        Some("a..b"),
    );
    write_status_note(
        &project_dir.join("PWF-0002.md"),
        "cancelled item",
        "cancelled",
        Some("2026-07-07"),
        Some("c..d"),
    );
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let reopened_done = store.reopen_item("PWF-0001").unwrap();
    let reopened_cancelled = store.reopen_item("PWF-0002").unwrap();

    assert_eq!(
        reopened_done,
        ReopenedItem {
            id: "PWF-0001".to_string(),
            project: "pwf".to_string(),
            already_active: false,
        }
    );
    assert!(!reopened_cancelled.already_active);
    for id in ["PWF-0001", "PWF-0002"] {
        let note = std::fs::read_to_string(project_dir.join(format!("{id}.md"))).unwrap();
        assert!(note.contains("status: active"), "{note}");
        assert!(!note.contains("completed:"), "{note}");
        assert!(!note.contains("commits:"), "{note}");
    }
    assert_eq!(
        std::fs::read_to_string(project_dir.join("pwf.md")).unwrap(),
        "---\nid: pwf\ntitle: pwf\n---\n\n- [ ] [[PWF-0001]]\n- [ ] [[PWF-0002]]\n"
    );
}

#[test]
fn reopen_item_restores_evicted_note_link_without_moving_the_file() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("pwf.md"), "# pwf\n").unwrap();
    write_status_note(
        &project_dir.join("PWF-0001.md"),
        "archived done",
        "done",
        Some("2026-07-07"),
        Some("a..b"),
    );
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let reopened = store.reopen_item("pwf-1").unwrap();

    assert_eq!(reopened.id, "PWF-0001");
    assert!(project_dir.join("PWF-0001.md").exists());
    assert!(!project_dir.join("_archive").exists());
    let index = std::fs::read_to_string(project_dir.join("pwf.md")).unwrap();
    assert!(index.contains("- [ ] [[PWF-0001]]"), "{index}");
}

#[test]
fn reopen_item_preserves_descriptive_filename() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("pwf.md"),
        "---\nid: pwf\ntitle: pwf\n---\n\n",
    )
    .unwrap();
    let note_path = project_dir.join("descriptive-name.md");
    std::fs::write(
        &note_path,
        "---\nid: PWF-0001\nstatus: done\ncompleted: 2026-07-12\ntitle: task\nproject: pwf\ncreated: 2026-07-01\n---\n\nbody\n",
    )
    .unwrap();
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let reopened = store.reopen_item("PWF-0001").unwrap();

    assert_eq!(reopened.id, "PWF-0001");
    assert!(note_path.exists());
    assert!(!project_dir.join("PWF-0001.md").exists());
}

#[test]
fn complete_item_preserves_legacy_inline_checkbox_behavior() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("pwf.md"),
        "- [ ] `legacy task` :: do the old thing\n",
    )
    .unwrap();
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let closed = store
        .complete_item(CompleteItemSpec {
            id: "pwf:1".to_string(),
            completed: "2026-07-07".to_string(),
            report: None,
            commits: None,
        })
        .unwrap();

    assert_eq!(closed.id, "pwf:1");
    assert_eq!(closed.title, "legacy task");
    assert_eq!(
        std::fs::read_to_string(project_dir.join("pwf.md")).unwrap(),
        "---\nid: pwf\ntitle: pwf\n---\n\n- [x] `legacy task` :: do the old thing ✅ 2026-07-07\n"
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
    store: ObsidianPendingWorkStore,
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
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));
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
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));
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

fn config_for_projects(notes_dir: &Path, projects: &[(&str, &str)]) -> pwf_core::config::Config {
    for (project, prefix) in projects {
        ensure_test_index_identity(
            &notes_dir.join(project).join(format!("{project}.md")),
            &prefix.to_ascii_lowercase(),
            project,
        );
    }
    let project_map = projects
        .iter()
        .map(|(name, _prefix)| {
            (
                (*name).to_string(),
                serde_json::Value::String(format!("/repo/{name}")),
            )
        })
        .collect::<serde_json::Map<String, serde_json::Value>>();
    let prefix_map = projects
        .iter()
        .map(|(name, prefix)| {
            (
                (*name).to_string(),
                serde_json::Value::String((*prefix).to_string()),
            )
        })
        .collect::<serde_json::Map<String, serde_json::Value>>();
    let config_json = serde_json::json!({
        "notesDir": notes_dir,
        "projects": project_map,
        "prefixes": prefix_map
    });
    from_json(&config_json.to_string(), None).unwrap()
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
fn note_with_project_finds_active_note_case_insensitively() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("test-project");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("test-project.md"),
        "---\nid: tst\ntitle: test-project\n---\n\n- [ ] [[TST-0001]]\n",
    )
    .unwrap();
    write_note(
        &project_dir.join("TST-0001.md"),
        "test task",
        "2026-07-01",
        None,
        None,
        None,
        "body",
    );

    let config_json = serde_json::json!({
        "notesDir": notes_dir,
        "projects": { "test-project": "/repo/test-project" },
        "prefixes": { "test-project": "TST" }
    });
    let config = from_json(&config_json.to_string(), None).unwrap();
    let store = ObsidianPendingWorkStore::new(config);

    let result = store.note_with_project("tst-0001");
    assert!(result.as_ref().unwrap().is_some());
    let (project, path) = result.unwrap().unwrap();
    assert_eq!(project, "test-project");
    assert!(path.ends_with("TST-0001.md"));
}

#[test]
fn note_with_project_returns_none_for_unknown_id() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("test-project");
    std::fs::create_dir_all(&project_dir).unwrap();

    let config_json = serde_json::json!({
        "notesDir": notes_dir,
        "projects": { "test-project": "/repo/test-project" },
        "prefixes": { "test-project": "TST" }
    });
    let config = from_json(&config_json.to_string(), None).unwrap();
    let store = ObsidianPendingWorkStore::new(config);

    let result = store.note_with_project("tst-9999");
    assert!(result.unwrap().is_none());
}
