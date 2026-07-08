use std::{fmt::Write as _, path::Path};

use pwf_application::{
    AddItemSpec, CancelItemSpec, ClosedItemAction, CompleteItemSpec, PendingWorkReadStore,
    PendingWorkResolveStore, PendingWorkWriteStore, ReopenedItem, ResolvePendingWorkOutput,
    StatusTransitionDiagnostics, UpdateItemSpec,
};
use pwf_core::config::from_json;
use pwf_domain::pending_work::OpenItem;

use super::{ObsidianPendingWorkStore, fs::path_str};

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
        "ship the adapter",
    );
    write_note(
        &project_dir.join("PWF-0002.md"),
        "human title",
        "2026-07-02",
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
        "follow up later",
    );

    let config_json = serde_json::json!({
        "notesDir": notes_dir,
        "projects": { "pwf": "/repo/pwf" },
        "prefixes": { "pwf": "PWF" }
    });
    let config = from_json(&config_json.to_string(), None).unwrap();
    let store = ObsidianPendingWorkStore::new(config);

    let got = store.open_items(None).unwrap();

    assert_eq!(
        got,
        vec![
            expected_item(&ItemExpectation {
                id: "PWF-0001",
                session: "normal title",
                prompt: "ship the adapter",
                repo: "/repo/pwf",
                project_dir: &project_dir,
                line: 2,
                section: None,
                prereq: Some("\"[[CFG-0001]]\""),
                effort: Some("2"),
                created: Some("2026-07-01"),
            }),
            expected_item(&ItemExpectation {
                id: "PWF-0002",
                session: "human title",
                prompt: "ask the human to verify",
                repo: "/repo/pwf",
                project_dir: &project_dir,
                line: 5,
                section: Some("Human"),
                prereq: None,
                effort: None,
                created: Some("2026-07-02"),
            }),
            expected_item(&ItemExpectation {
                id: "PWF-0003",
                session: "future title",
                prompt: "follow up later",
                repo: "/repo/pwf",
                project_dir: &project_dir,
                line: 8,
                section: Some("Future"),
                prereq: None,
                effort: Some("4"),
                created: Some("2026-07-03"),
            }),
        ]
    );
}

fn write_note(
    path: &Path,
    title: &str,
    created: &str,
    prereq: Option<&str>,
    effort: Option<&str>,
    body: &str,
) {
    let mut note =
        format!("---\nstatus: active\ntitle: {title}\nproject: pwf\ncreated: {created}\n");
    if let Some(prereq) = prereq {
        let _ = writeln!(note, "prereq: {prereq}");
    }
    if let Some(effort) = effort {
        let _ = writeln!(note, "effort: {effort}");
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
        })
        .unwrap();

    assert_eq!(added.id, "PWF-0001");
    assert_eq!(added.project, "pwf");
    assert_eq!(added.title, "ship adapter");
    assert_eq!(added.created_section, Some("Human".to_string()));
    let note = std::fs::read_to_string(notes_dir.join("pwf/PWF-0001.md")).unwrap();
    assert!(note.contains("status: active"), "{note}");
    assert!(note.contains("title: ship adapter"), "{note}");
    assert!(note.contains("created: 2026-07-07"), "{note}");
    assert!(note.contains("prereq: \"[[PWF-0001]]\""), "{note}");
    assert!(note.contains("effort: 2"), "{note}");
    assert!(note.contains("## Goals\n- Ship the adapter"), "{note}");
    let index = std::fs::read_to_string(notes_dir.join("pwf/pwf.md")).unwrap();
    assert_eq!(index, "\n\n## Human\n\n- [ ] [[PWF-0001]]\n");
}

#[test]
fn add_item_index_write_error_preserves_created_section_diagnostic() {
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
        })
        .unwrap_err();

    assert!(err.to_string().starts_with("Failed to write index file: "));
    assert_eq!(err.created_section_diagnostic(), Some(("pwf", "Human")));
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
        "already exists",
    );
    write_note(
        &project_dir.join("PWF-0002.md"),
        "old title",
        "2026-07-02",
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
        ""
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
fn resolve_item_show_returns_open_note_markdown_without_created_key() {
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
        "## Goals\n- body",
    );
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let got = store.resolve_item("PWF-0001", true).unwrap();

    assert_eq!(
        got,
        ResolvePendingWorkOutput::NoteMarkdown(
            "---\nstatus: active\ntitle: active task\nproject: pwf\n---\n\n## Goals\n- body\n"
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
            "---\nstatus: done\ntitle: done task\nproject: pwf\ncompleted: 2026-07-07\n---\n\nbody\n"
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
            "---\nstatus: done\ntitle: done task\nproject: pwf\ncompleted: 2026-07-07\n---\n\nbody\n"
                .to_string(),
        )
    );
}

#[test]
fn resolve_item_show_finds_archived_note_case_insensitively() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let archive_dir = notes_dir.join("pwf/_archive");
    std::fs::create_dir_all(&archive_dir).unwrap();
    write_status_note(
        &archive_dir.join("PWF-0002.md"),
        "archived task",
        "cancelled",
        Some("2026-07-07"),
        Some("a..b"),
    );
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let got = store.resolve_item("pwf-0002", true).unwrap();

    assert_eq!(
        got,
        ResolvePendingWorkOutput::NoteMarkdown(
            "---\nstatus: cancelled\ntitle: archived task\nproject: pwf\ncompleted: 2026-07-07\ncommits: \"a..b\"\n---\n\nbody\n"
                .to_string(),
        )
    );
}

#[test]
fn resolve_item_show_finds_archived_note_with_shorthand_id() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let archive_dir = notes_dir.join("pwf/_archive");
    std::fs::create_dir_all(&archive_dir).unwrap();
    write_status_note(
        &archive_dir.join("PWF-0002.md"),
        "archived task",
        "cancelled",
        Some("2026-07-07"),
        Some("a..b"),
    );
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let got = store.resolve_item("pwf-2", true).unwrap();

    assert_eq!(
        got,
        ResolvePendingWorkOutput::NoteMarkdown(
            "---\nstatus: cancelled\ntitle: archived task\nproject: pwf\ncompleted: 2026-07-07\ncommits: \"a..b\"\n---\n\nbody\n"
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
        "---\nstatus: active\ntitle: alpha task\nproject: alpha\ncreated: 2026-07-01\n---\n\nbody\n",
    )
    .unwrap();
    std::fs::write(
        beta_dir.join("PWF-0001.md"),
        "---\nstatus: active\ntitle: beta task\nproject: beta\ncreated: 2026-07-02\n---\n\nbody\n",
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
        "- [x] [[PWF-0001]] ✅ 2026-07-07\n"
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
fn complete_item_rotates_done_queue_and_archives_evicted_notes() {
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
    assert!(!project_dir.join("PWF-0001.md").exists());
    assert!(project_dir.join("_archive/PWF-0001.md").exists());
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
        "- [ ] [[PWF-0001]]\n- [ ] [[PWF-0002]]\n"
    );
}

#[test]
fn reopen_item_restores_evicted_archive_note_and_readds_index_link() {
    let temp = tempfile::tempdir().unwrap();
    let notes_dir = temp.path().join("notes");
    let project_dir = notes_dir.join("pwf");
    let archive_dir = project_dir.join("_archive");
    std::fs::create_dir_all(&archive_dir).unwrap();
    std::fs::write(project_dir.join("pwf.md"), "# pwf\n").unwrap();
    write_status_note(
        &archive_dir.join("PWF-0001.md"),
        "archived done",
        "done",
        Some("2026-07-07"),
        Some("a..b"),
    );
    let store = ObsidianPendingWorkStore::new(config_for_notes(&notes_dir));

    let reopened = store.reopen_item("pwf-1").unwrap();

    assert_eq!(reopened.id, "PWF-0001");
    assert!(project_dir.join("PWF-0001.md").exists());
    assert!(!archive_dir.join("PWF-0001.md").exists());
    let index = std::fs::read_to_string(project_dir.join("pwf.md")).unwrap();
    assert!(index.contains("- [ ] [[PWF-0001]]"), "{index}");
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
        "- [x] `legacy task` :: do the old thing ✅ 2026-07-07\n"
    );
}

fn config_for_notes(notes_dir: &Path) -> pwf_core::config::Config {
    let config_json = serde_json::json!({
        "notesDir": notes_dir,
        "projects": { "pwf": "/repo/pwf" },
        "prefixes": { "pwf": "PWF" }
    });
    from_json(&config_json.to_string(), None).unwrap()
}

fn config_for_projects(notes_dir: &Path, projects: &[(&str, &str)]) -> pwf_core::config::Config {
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

fn write_status_note(
    path: &Path,
    title: &str,
    status: &str,
    completed: Option<&str>,
    commits: Option<&str>,
) {
    let mut note =
        format!("---\nstatus: {status}\ntitle: {title}\nproject: pwf\ncreated: 2026-07-01\n");
    if let Some(completed) = completed {
        let _ = writeln!(note, "completed: {completed}");
    }
    if let Some(commits) = commits {
        let _ = writeln!(note, "commits: \"{commits}\"");
    }
    note.push_str("---\n\nbody\n");
    std::fs::write(path, note).unwrap();
}
