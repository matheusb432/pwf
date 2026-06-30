use std::{
    fs,
    path::{Path, PathBuf},
};

use pwf::engines::handoff;

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

fn tmpdir(prefix: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("{prefix}_{}", nanos()));
    fs::create_dir_all(&d).unwrap();
    d
}

fn write_config(dir: &Path, repo: &Path, notes: &Path) -> PathBuf {
    let cfg_path = dir.join("config.json");
    let json = format!(
        r#"{{ "notesDir": "{}", "projects": {{ "test-project": "{}" }}, "prefixes": {{ "test-project": "TST" }} }}"#,
        notes.to_string_lossy().replace('\\', "/"),
        repo.to_string_lossy().replace('\\', "/")
    );
    fs::write(&cfg_path, &json).unwrap();
    cfg_path
}

fn write_empty_config(dir: &Path) -> PathBuf {
    let cfg_path = dir.join("config.json");
    fs::write(
        &cfg_path,
        r#"{ "notesDir": "/tmp/notes", "projects": {}, "prefixes": {} }"#,
    )
    .unwrap();
    cfg_path
}

fn parse_args(tokens: &[&str]) -> pwf::cli::Args {
    let mut v = vec!["handoff".to_string()];
    v.extend(tokens.iter().map(|s| s.to_string()));
    pwf::command::parse_argv(v).unwrap().1
}

// A few handoff tests drive a pending-work command (e.g. `check`) as a setup
// step; clap is engine-rooted, so those parse under the `pw` engine.
fn parse_pw(tokens: &[&str]) -> pwf::cli::Args {
    let mut v = vec!["pw".to_string()];
    v.extend(tokens.iter().map(|s| s.to_string()));
    pwf::command::parse_argv(v).unwrap().1
}

fn pw_stub_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pw-stub.sh")
}

// ── Task 18: scaffold + slug ────────────────────────────────────────────────

#[test]
fn scaffold_matches_expected_shape_without_pw() {
    let s = handoff::scaffold("Managed Flow", "test-project", "2026-01-01", None);
    let expected_prefix =
        "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\n---\n\n# Managed Flow\n";
    assert!(
        s.starts_with(expected_prefix),
        "scaffold prefix mismatch:\ngot:      {:?}\nexpected: {:?}",
        &s[..expected_prefix.len().min(s.len())],
        expected_prefix
    );
    assert!(!s.contains("pw:"));
    // em-dash
    assert!(s.contains('\u{2014}'));
    // goal placeholder
    assert!(s.contains("- [ ] <task title> :: <task description>"));
    // Next steps
    assert!(s.contains("## Next steps\n-\n"));
}

#[test]
fn scaffold_with_pw_inserted_after_created() {
    let s = handoff::scaffold(
        "Managed Flow",
        "test-project",
        "2026-01-01",
        Some("TST-0001"),
    );
    assert!(
        s.contains("created: 2026-01-01\npw: TST-0001\n---"),
        "pw line not after created: {:?}",
        s
    );
}

#[test]
fn slug_converts_title() {
    assert_eq!(handoff::slug("Managed Flow"), "managed-flow");
    assert_eq!(handoff::slug("  Hello World!! "), "hello-world");
    assert_eq!(handoff::slug(""), "handoff");
}

// ── Task 19: ledger refresh ─────────────────────────────────────────────────

#[test]
fn refresh_counts_active_only() {
    let stage = tmpdir("hf_refresh");
    let repo = stage.join("repo");
    let handoff_dir = repo.join("docs/handoffs");
    fs::create_dir_all(&handoff_dir).unwrap();

    // Done handoff
    fs::write(
        handoff_dir.join("2026-01-01-done.md"),
        "---\nstatus: done\nproject: test-project\ncreated: 2026-01-01\ncompleted: 2026-01-01\npw: TST-0001\n---\n\n# Done Handoff\n\n## Goals\n- [x] closed task :: complete work\n\n## Context\n\nDone handoffs are excluded from the active ledger.\n",
    ).unwrap();

    // Active handoff
    fs::write(
        handoff_dir.join("2026-01-02-active.md"),
        "---\nstatus: active\nproject: test-project\ncreated: 2026-01-02\npw: TST-0002\n---\n\n# Active Followup\n\n## Goals\n- [x] inspect state :: read files\n- [ ] update state :: write files\n\n## Context\n\nOnly this active handoff should appear in the ledger.\n",
    ).unwrap();

    let notes = stage.join("notes");
    let cfg = write_config(&stage, &repo, &notes);
    let args = parse_args(&[
        "refresh",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-01",
    ]);
    let out = handoff::run(&args).unwrap();
    assert!(
        out.starts_with("LEDGER refreshed."),
        "expected refresh text, got: {out}"
    );

    let ledger = fs::read_to_string(handoff_dir.join("LEDGER.md")).unwrap();
    assert!(ledger.contains("TST-0002"), "ledger missing TST-0002");
    assert!(ledger.contains("1/2"), "ledger missing goals 1/2");
    assert!(
        !ledger.contains("TST-0001"),
        "done handoff should not appear"
    );

    // active-only count proxy: one table data row (header excluded; `---` separator excluded)
    let active_rows = ledger
        .lines()
        .filter(|l| l.starts_with("| ") && !l.contains("---"))
        .count()
        - 1; // subtract the `| ID | Handoff | … |` header row
    assert_eq!(
        active_rows, 1,
        "expected exactly 1 active handoff row, ledger:\n{ledger}"
    );
}

// ── Task 20: new ───────────────────────────────────────────────────────────

#[test]
fn new_unmanaged_no_pw() {
    let stage = tmpdir("hf_new_unm");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let cfg = write_empty_config(&stage);
    let args = parse_args(&[
        "new",
        "--title",
        "Unmanaged Flow",
        "--slug",
        "unmanaged-flow",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-01",
    ]);
    let out = handoff::run(&args).unwrap();
    assert!(
        out.starts_with("Created handoff "),
        "expected created text, got: {out}"
    );
    assert!(
        out.contains("2026-01-01-unmanaged-flow.md"),
        "file path missing: {out}"
    );

    let file_path = repo.join("docs/handoffs/2026-01-01-unmanaged-flow.md");
    assert!(file_path.exists(), "handoff file not created");
    let content = fs::read_to_string(&file_path).unwrap();
    assert!(
        content.contains("project: repo"),
        "project label should be repo dir leaf"
    );
    assert!(!content.contains("pw:"), "unmanaged should have no pw line");
}

#[test]
fn new_managed_calls_pw_stub() {
    // This test uses the actual pw-stub.sh fixture to verify the spawn path.
    let stage = tmpdir("hf_new_mgd");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    let cfg = write_config(&stage, &repo, &notes);

    let stub = pw_stub_path();
    let args = parse_args(&[
        "new",
        "--title",
        "Managed Flow",
        "--slug",
        "managed-flow",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-01",
        "--pending-work-script",
        stub.to_str().unwrap(),
    ]);
    let out = handoff::run(&args).unwrap();
    assert!(
        out.starts_with("Created handoff "),
        "expected created text, got: {out}"
    );
    assert!(out.contains("TST-0001"), "pw id missing from output: {out}");

    let file_path = repo.join("docs/handoffs/2026-01-01-managed-flow.md");
    let content = fs::read_to_string(&file_path).unwrap();
    assert!(
        content.contains("pw: TST-0001"),
        "pw not in file: {content}"
    );
}

#[test]
fn new_managed_links_pw_in_process_without_script() {
    // Production path (no --pending-work-script): handoff calls the pending-work engine
    // in-process to allocate + link the work item, mirroring the PS default script.
    let stage = tmpdir("hf_new_inproc");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    let cfg = write_config(&stage, &repo, &notes);
    let args = parse_args(&[
        "new",
        "--title",
        "Managed Flow",
        "--slug",
        "managed-flow",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-01",
        // no --pending-work-script → in-process fallback
    ]);
    let out = handoff::run(&args).unwrap();
    assert!(
        out.starts_with("Created handoff "),
        "expected created text, got: {out}"
    );
    assert!(out.contains("TST-0001"), "pw id missing from output: {out}");
    let content =
        fs::read_to_string(repo.join("docs/handoffs/2026-01-01-managed-flow.md")).unwrap();
    assert!(
        content.contains("pw: TST-0001"),
        "pw not in file: {content}"
    );
    assert!(
        notes.join("test-project/TST-0001.md").exists(),
        "work-item note not created in the vault"
    );
}

// ── Task 21: done / cancel ──────────────────────────────────────────────────

#[test]
fn done_archives_and_updates_frontmatter() {
    let stage = tmpdir("hf_done");
    let repo = stage.join("repo");
    let handoff_dir = repo.join("docs/handoffs");
    fs::create_dir_all(&handoff_dir).unwrap();
    fs::write(
        handoff_dir.join("2026-01-01-managed-flow.md"),
        "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n\n## Goals\n- [x] draft plan :: write implementation plan\n- [ ] verify plan :: run checks\n\n## Context\n\nFinish the active handoff.\n",
    ).unwrap();
    // initial LEDGER
    fs::write(handoff_dir.join("LEDGER.md"), "# stale\n").unwrap();

    let notes = stage.join("notes");
    let cfg = write_config(&stage, &repo, &notes);
    let stub = pw_stub_path();
    let args = parse_args(&[
        "done",
        "--id",
        "TST-0001",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-01",
        "--no-commit",
        "--pending-work-script",
        stub.to_str().unwrap(),
    ]);
    let out = handoff::run(&args).unwrap();
    assert!(
        out.contains("done handoff "),
        "expected 'done handoff' text, got: {out}"
    );

    let archived = repo.join("docs/handoffs/archived/2026-01-01-managed-flow.md");
    assert!(archived.exists(), "archived file should exist");
    let content = fs::read_to_string(&archived).unwrap();
    assert!(content.contains("status: done"), "status not set to done");
    assert!(
        content.contains("completed: 2026-01-01"),
        "completed not set"
    );
    // frontmatter order: status, completed appear before project/created
    let status_pos = content.find("status:").unwrap();
    let completed_pos = content.find("completed:").unwrap();
    let project_pos = content.find("project:").unwrap();
    assert!(
        status_pos < completed_pos,
        "completed should be after status"
    );
    assert!(
        completed_pos < project_pos,
        "completed should be before project"
    );
}

#[test]
fn cancel_archives_with_reason() {
    let stage = tmpdir("hf_cancel");
    let repo = stage.join("repo");
    let handoff_dir = repo.join("docs/handoffs");
    fs::create_dir_all(&handoff_dir).unwrap();
    fs::write(
        handoff_dir.join("2026-01-01-cancel-me.md"),
        "---\nstatus: active\nproject: config-handler\ncreated: 2026-01-01\n---\n\n# Cancel me\n\n## Goals\n- [ ] x :: y\n",
    ).unwrap();
    fs::write(
        handoff_dir.join("LEDGER.md"),
        "# Handoff ledger - active only\n\n| ID | Handoff | Goals | Created |\n| --- | --- | --- | --- |\n| 2026-01-01-cancel-me | [Cancel me](2026-01-01-cancel-me.md) | 0/1 | 2026-01-01 |\n",
    ).unwrap();

    let cfg = write_empty_config(&stage);
    let args = parse_args(&[
        "cancel",
        "--id",
        "cancel-me",
        "--reason",
        "scope dropped",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-01",
        "--no-commit",
    ]);
    let out = handoff::run(&args).unwrap();
    assert!(
        out.contains("cancelled handoff "),
        "expected 'cancelled handoff' text, got: {out}"
    );

    let archived = repo.join("docs/handoffs/archived/2026-01-01-cancel-me.md");
    assert!(archived.exists(), "archived file should exist");
    let content = fs::read_to_string(&archived).unwrap();
    assert!(content.contains("status: cancelled"), "status not set");
    assert!(
        content.contains("completed: 2026-01-01"),
        "completed not set"
    );
    assert!(
        content.contains("> Cancelled: scope dropped"),
        "reason not appended: {content}"
    );
}

// ── PWF-0012: idempotent + atomic done ──────────────────────────────────────

#[test]
fn done_skips_already_checked_pw_item_and_archives() {
    let stage = tmpdir("hf_done_idem");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    let cfg = write_config(&stage, &repo, &notes);

    // Create handoff + linked open item TST-0001 (in-process production path).
    let new_args = parse_args(&[
        "new",
        "--title",
        "Managed Flow",
        "--slug",
        "managed-flow",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-01",
    ]);
    handoff::run(&new_args).unwrap();

    // Check the linked item directly — the live reproduction step.
    let check_args = parse_pw(&[
        "check",
        "--id",
        "TST-0001",
        "--config-path",
        cfg.to_str().unwrap(),
        "--date",
        "2026-01-01",
    ]);
    pwf::engines::pending_work::run_args(&check_args).unwrap();

    // done must treat the already-checked item as success (skip with a note).
    let done_args = parse_args(&[
        "done",
        "--id",
        "TST-0001",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-02",
        "--no-commit",
    ]);
    let out = handoff::run(&done_args)
        .unwrap_or_else(|e| panic!("done should succeed when pw item already checked: {e}"));
    assert!(
        out.contains("done handoff "),
        "expected 'done handoff' text, got: {out}"
    );
    assert!(
        out.contains("already checked"),
        "expected skipped note in output, got: {out}"
    );

    let active = repo.join("docs/handoffs/2026-01-01-managed-flow.md");
    let archived = repo.join("docs/handoffs/archived/2026-01-01-managed-flow.md");
    assert!(archived.exists(), "handoff should be archived");
    assert!(!active.exists(), "active copy should be gone");
    let content = fs::read_to_string(&archived).unwrap();
    assert!(
        content.contains("status: done"),
        "status not set: {content}"
    );
}

// ── PWF-0054: reopen ─────────────────────────────────────────────────────────

#[test]
fn reopen_un_archives_handoff_and_reopens_linked_pw_item() {
    // The full inverse of `done`: new → done → reopen must flip BOTH the handoff and
    // its linked pw item back to active, via the in-process production path.
    let stage = tmpdir("hf_reopen_paired");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    let cfg = write_config(&stage, &repo, &notes);

    let new_args = parse_args(&[
        "new",
        "--title",
        "Managed Flow",
        "--slug",
        "managed-flow",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-01",
    ]);
    handoff::run(&new_args).unwrap();

    let done_args = parse_args(&[
        "done",
        "--id",
        "TST-0001",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-02",
        "--no-commit",
    ]);
    handoff::run(&done_args).unwrap();

    // Preconditions: handoff archived (done), pw item closed.
    let archived = repo.join("docs/handoffs/archived/2026-01-01-managed-flow.md");
    assert!(archived.exists(), "precondition: handoff archived");
    let pw_note = notes.join("test-project/TST-0001.md");
    assert!(
        fs::read_to_string(&pw_note)
            .unwrap()
            .contains("status: done"),
        "precondition: pw item done"
    );

    // Reopen — by the linked pw id, which find_archived matches via frontmatter.
    let reopen_args = parse_args(&[
        "reopen",
        "--id",
        "TST-0001",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--no-commit",
    ]);
    let out = handoff::run(&reopen_args).unwrap();
    assert!(
        out.contains("reopened handoff "),
        "expected 'reopened handoff', got: {out}"
    );
    assert!(
        out.contains("pw: reopened TST-0001"),
        "pw note missing: {out}"
    );

    // Handoff: un-archived, active, no completed stamp.
    let active = repo.join("docs/handoffs/2026-01-01-managed-flow.md");
    assert!(active.exists(), "handoff should be back in active dir");
    assert!(!archived.exists(), "archived copy should be gone");
    let hf = fs::read_to_string(&active).unwrap();
    assert!(hf.contains("status: active"), "handoff status: {hf}");
    assert!(!hf.contains("completed:"), "completed lingered: {hf}");

    // Linked pw item: reopened (note active, no provenance, index link open).
    let note = fs::read_to_string(&pw_note).unwrap();
    assert!(note.contains("status: active"), "pw note status: {note}");
    assert!(
        !note.contains("completed:"),
        "pw completed lingered: {note}"
    );
    let index = fs::read_to_string(notes.join("test-project/test-project.md")).unwrap();
    assert!(
        index.contains("- [ ] [[TST-0001]]"),
        "pw index link not reopened: {index}"
    );

    // LEDGER row restored (handoff is active again).
    let ledger = fs::read_to_string(repo.join("docs/handoffs/LEDGER.md")).unwrap();
    assert!(
        ledger.contains("2026-01-01-managed-flow.md"),
        "LEDGER row not restored: {ledger}"
    );
}

#[test]
fn reopen_conflict_when_active_file_exists_leaves_archived_intact() {
    let stage = tmpdir("hf_reopen_conflict");
    let repo = stage.join("repo");
    let handoff_dir = repo.join("docs/handoffs");
    let archive_dir = handoff_dir.join("archived");
    fs::create_dir_all(&archive_dir).unwrap();
    let archived = archive_dir.join("2026-01-01-managed-flow.md");
    fs::write(
        &archived,
        "---\nstatus: done\ncompleted: 2026-01-02\nproject: test-project\ncreated: 2026-01-01\n---\n\n# Managed Flow\n",
    )
    .unwrap();
    // An active file of the same name already exists → conflict.
    let active = handoff_dir.join("2026-01-01-managed-flow.md");
    fs::write(&active, "frozen active\n").unwrap();

    let cfg = write_empty_config(&stage);
    let args = parse_args(&[
        "reopen",
        "--id",
        "managed-flow",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--no-commit",
    ]);
    let err = handoff::run(&args).unwrap_err();
    assert!(
        err.contains("Handoff already exists"),
        "unexpected error: {err}"
    );
    // No partial state: archived untouched, active untouched.
    assert!(archived.exists(), "archived must survive a conflict");
    assert_eq!(fs::read_to_string(&active).unwrap(), "frozen active\n");
}

#[test]
fn reopen_unknown_id_errors() {
    let stage = tmpdir("hf_reopen_missing");
    let repo = stage.join("repo");
    fs::create_dir_all(repo.join("docs/handoffs/archived")).unwrap();
    let cfg = write_empty_config(&stage);
    let args = parse_args(&[
        "reopen",
        "--id",
        "nope",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--no-commit",
    ]);
    let err = handoff::run(&args).unwrap_err();
    assert!(
        err.contains("No archived handoff found"),
        "unexpected error: {err}"
    );
}

#[test]
fn done_dest_conflict_leaves_no_partial_state() {
    let stage = tmpdir("hf_done_atomic");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    let cfg = write_config(&stage, &repo, &notes);

    // Create handoff + linked open item TST-0001 (in-process production path).
    let new_args = parse_args(&[
        "new",
        "--title",
        "Managed Flow",
        "--slug",
        "managed-flow",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-01",
    ]);
    handoff::run(&new_args).unwrap();

    // Force a late failure: the archive destination already exists.
    let active = repo.join("docs/handoffs/2026-01-01-managed-flow.md");
    let before = fs::read_to_string(&active).unwrap();
    let archived_dir = repo.join("docs/handoffs/archived");
    fs::create_dir_all(&archived_dir).unwrap();
    fs::write(
        archived_dir.join("2026-01-01-managed-flow.md"),
        "frozen archive\n",
    )
    .unwrap();

    let done_args = parse_args(&[
        "done",
        "--id",
        "TST-0001",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-02",
        "--no-commit",
    ]);
    let err = handoff::run(&done_args).unwrap_err();
    assert!(
        err.contains("Archived handoff already exists"),
        "unexpected error: {err}"
    );

    // No partial state: active file untouched, linked pw item still open.
    let after = fs::read_to_string(&active).unwrap();
    assert_eq!(before, after, "active handoff must be untouched on failure");
    let list_args = parse_args(&[
        "list",
        "--config-path",
        cfg.to_str().unwrap(),
        "--date",
        "2026-01-02",
    ]);
    let out = pwf::engines::pending_work::run_args(&list_args).unwrap();
    assert!(
        out.contains("TST-0001"),
        "pw item must remain open on failure: {out}"
    );
}

#[test]
fn refresh_archives_stranded_non_active_handoffs() {
    let stage = tmpdir("hf_refresh_sweep");
    let repo = stage.join("repo");
    let handoff_dir = repo.join("docs/handoffs");
    fs::create_dir_all(&handoff_dir).unwrap();
    fs::write(
        handoff_dir.join("2026-01-01-stranded.md"),
        "---\nstatus: done\ncompleted: 2026-01-02\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Stranded\n\n## Goals\n- [x] a :: b\n",
    )
    .unwrap();
    fs::write(
        handoff_dir.join("2026-01-02-active.md"),
        "---\nstatus: active\nproject: test-project\ncreated: 2026-01-02\npw: TST-0002\n---\n\n# Active\n\n## Goals\n- [ ] c :: d\n",
    )
    .unwrap();
    // No status: key — a draft, not the sweep's business.
    fs::write(handoff_dir.join("2026-01-03-draft.md"), "# Draft notes\n").unwrap();

    let cfg = write_empty_config(&stage);
    let args = parse_args(&[
        "refresh",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-03",
    ]);
    let out = handoff::run(&args).unwrap();
    assert!(
        out.starts_with("LEDGER refreshed."),
        "expected refresh text, got: {out}"
    );
    assert!(
        out.contains("archived 1 stranded"),
        "expected archive count in output, got: {out}"
    );

    assert!(
        handoff_dir.join("archived/2026-01-01-stranded.md").exists(),
        "stranded file should be moved to archived/"
    );
    assert!(
        !handoff_dir.join("2026-01-01-stranded.md").exists(),
        "stranded file should leave the active dir"
    );
    assert!(
        handoff_dir.join("2026-01-03-draft.md").exists(),
        "draft without status must be untouched"
    );
    let ledger = fs::read_to_string(handoff_dir.join("LEDGER.md")).unwrap();
    assert!(!ledger.contains("TST-0001"), "swept handoff not in ledger");
    assert!(ledger.contains("TST-0002"), "active handoff in ledger");
}

#[test]
fn refresh_reports_archive_conflict_without_moving() {
    let stage = tmpdir("hf_refresh_conflict");
    let repo = stage.join("repo");
    let handoff_dir = repo.join("docs/handoffs");
    fs::create_dir_all(handoff_dir.join("archived")).unwrap();
    fs::write(
        handoff_dir.join("2026-01-01-clash.md"),
        "---\nstatus: done\nproject: test-project\ncreated: 2026-01-01\n---\n\n# Clash\n",
    )
    .unwrap();
    fs::write(
        handoff_dir.join("archived/2026-01-01-clash.md"),
        "frozen archive\n",
    )
    .unwrap();

    let cfg = write_empty_config(&stage);
    let args = parse_args(&[
        "refresh",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-02",
    ]);
    let out = handoff::run(&args).unwrap();
    assert!(
        out.starts_with("LEDGER refreshed."),
        "expected refresh text, got: {out}"
    );
    assert!(
        !out.contains("archived 1"),
        "conflicting file must not be counted as archived, got: {out}"
    );
    assert!(
        out.contains("2026-01-01-clash.md"),
        "conflict filename missing from output: {out}"
    );
    assert!(
        handoff_dir.join("2026-01-01-clash.md").exists(),
        "conflicting file stays in place"
    );
    assert_eq!(
        fs::read_to_string(handoff_dir.join("archived/2026-01-01-clash.md")).unwrap(),
        "frozen archive\n",
        "existing archive must never be overwritten"
    );
}

// ── PWF-0017 Phase 4: done forwards --commits/--review to the linked pw item ─

#[test]
fn done_forwards_commits_and_review_to_linked_pw_item() {
    let stage = tmpdir("hf_done_provenance");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    let cfg = write_config(&stage, &repo, &notes);

    // Create handoff + linked OPEN file-model item TST-0001 (in-process path).
    let new_args = parse_args(&[
        "new",
        "--title",
        "Managed Flow",
        "--slug",
        "managed-flow",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-01",
    ]);
    handoff::run(&new_args).unwrap();

    // done with provenance, in-process (no --pending-work-script → inprocess_pw_check).
    let done_args = parse_args(&[
        "done",
        "--id",
        "TST-0001",
        "--commits",
        "aaaa..bbbb",
        "--review",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-02",
        "--no-commit",
    ]);
    handoff::run(&done_args).unwrap();

    // (1) The linked pw item file records the commit range.
    let item = fs::read_to_string(notes.join("test-project/TST-0001.md")).unwrap();
    assert!(
        item.contains("commits: \"aaaa..bbbb\""),
        "commit range not forwarded to linked item: {item}"
    );

    // (2) A ## Human review task was spawned in the project, prepped with the
    // range-scoped git-tools diff commands.
    let index = fs::read_to_string(notes.join("test-project/test-project.md")).unwrap();
    assert!(index.contains("## Human"), "no Human section: {index}");
    let human = fs::read_to_string(notes.join("test-project/TST-0002.md")).unwrap();
    assert!(
        human.contains("git-tools diff aaaa..bbbb"),
        "review task missing range-scoped diff: {human}"
    );
    assert!(
        human.contains("git-tools diff-subrepos"),
        "review task missing diff-subrepos: {human}"
    );

    // (3) The handoff was archived as done, as usual.
    let archived = repo.join("docs/handoffs/archived/2026-01-02-managed-flow.md");
    let archived = if archived.exists() {
        archived
    } else {
        repo.join("docs/handoffs/archived/2026-01-01-managed-flow.md")
    };
    assert!(archived.exists(), "handoff should be archived");
    let content = fs::read_to_string(&archived).unwrap();
    assert!(
        content.contains("status: done"),
        "status not done: {content}"
    );
}

// ── Task 22: list + refresh dispatch ───────────────────────────────────────

#[test]
fn list_returns_ledger_content() {
    let stage = tmpdir("hf_list");
    let repo = stage.join("repo");
    let handoff_dir = repo.join("docs/handoffs");
    fs::create_dir_all(&handoff_dir).unwrap();
    let ledger_content = "# Handoff ledger - active only\n\n| ID | Handoff | Goals | Created |\n| --- | --- | --- | --- |\n| CFG-0001 | [Alpha](2026-01-01-alpha.md) | 1/2 | 2026-01-01 |\n";
    fs::write(handoff_dir.join("LEDGER.md"), ledger_content).unwrap();

    let cfg = write_empty_config(&stage);
    let args = parse_args(&[
        "list",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-01",
    ]);
    let out = handoff::run(&args).unwrap();
    assert_eq!(
        out, ledger_content,
        "list should return ledger content verbatim"
    );
}

#[test]
fn list_no_ledger() {
    let stage = tmpdir("hf_list_empty");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let cfg = write_empty_config(&stage);
    let args = parse_args(&[
        "list",
        "--config-path",
        cfg.to_str().unwrap(),
        "--repo-root",
        repo.to_str().unwrap(),
        "--date",
        "2026-01-01",
    ]);
    let out = handoff::run(&args).unwrap();
    assert_eq!(out, "No active handoffs (LEDGER.md not found).");
}
