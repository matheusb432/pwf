use std::{
    fs,
    path::{Path, PathBuf},
};

use pwf::engines::{handoff, pending_work as pwk};

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
    v.extend(tokens.iter().map(std::string::ToString::to_string));
    pwf::command::parse_argv(v).unwrap().1
}

fn pw_stub_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pw-stub.sh")
}

/// An allocator stub that runs successfully but emits stdout `parse_added_id`
/// can't extract an id from — used to drive `handoff add`'s scaffold-cleanup
/// path when pw allocation fails after the scaffold file is already written.
fn pw_stub_garbage_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pw-stub-garbage.sh")
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
        "pw line not after created: {s:?}"
    );
}

#[test]
fn slug_converts_title() {
    assert_eq!(handoff::slug("Managed Flow"), "managed-flow");
    assert_eq!(handoff::slug("  Hello World!! "), "hello-world");
    assert_eq!(handoff::slug(""), "handoff");
}

// ── Task 20 / PWF-0117: add ─────────────────────────────────────────────────

#[test]
fn add_unmanaged_repo_errors() {
    let stage = tmpdir("hf_add_unm");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let cfg = write_empty_config(&stage);
    let args = parse_args(&[
        "add",
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
    let err = handoff::run(&args).unwrap_err();
    assert!(
        err.contains("this repo is not a managed project"),
        "unexpected error: {err}"
    );
    assert!(
        err.contains("register it in the pwf config"),
        "unexpected error: {err}"
    );
    assert!(
        err.contains(&repo.to_string_lossy().into_owned()),
        "error should name the repo root: {err}"
    );

    let file_path = repo.join("docs/handoffs/2026-01-01-unmanaged-flow.md");
    assert!(
        !file_path.exists(),
        "unmanaged repo must not write a handoff file"
    );
    // An unmanaged repo must get no filesystem writes at all — not even an
    // empty `docs/handoffs/` — so the `UnmanagedRepo` check must run before
    // any directory is created.
    assert!(
        !repo.join("docs/handoffs").exists(),
        "unmanaged repo must not have docs/handoffs/ created at all"
    );
}

#[test]
fn add_managed_calls_pw_stub() {
    // This test uses the actual pw-stub.sh fixture to verify the spawn path.
    let stage = tmpdir("hf_new_mgd");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    let cfg = write_config(&stage, &repo, &notes);

    let stub = pw_stub_path();
    let args = parse_args(&[
        "add",
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
fn add_removes_orphan_scaffold_when_pw_allocation_fails() {
    // The scaffold is written before pw allocation (`--continue-handoff` needs
    // it on disk), so a failed allocation must best-effort delete it — else a
    // handoff with no `pw:` link is stranded on disk with nothing pointing at
    // it (PWF-0117 final-review item 4).
    let stage = tmpdir("hf_add_orphan_scaffold");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    let cfg = write_config(&stage, &repo, &notes);

    let stub = pw_stub_garbage_path();
    let args = parse_args(&[
        "add",
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
    let err = handoff::run(&args).unwrap_err();

    assert!(err.contains("pw-add output parse error"), "got: {err}");
    let file_path = repo.join("docs/handoffs/2026-01-01-managed-flow.md");
    assert!(
        !file_path.exists(),
        "orphan scaffold must be removed when pw allocation fails: {}",
        file_path.display()
    );
}

#[test]
fn add_managed_links_pw_in_process_without_script() {
    // Production path (no --pending-work-script): handoff calls the pending-work engine
    // in-process to allocate + link the work item, mirroring the PS default script.
    let stage = tmpdir("hf_new_inproc");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    let cfg = write_config(&stage, &repo, &notes);
    let args = parse_args(&[
        "add",
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
    let note_path = notes.join("test-project/TST-0001.md");
    assert!(
        note_path.exists(),
        "work-item note not created in the vault"
    );
    let note = fs::read_to_string(&note_path).unwrap();
    assert!(
        note.contains("tags: [handoff]"),
        "handoff tag missing: {note}"
    );

    // `invoke_add` (above) writes the scaffold directly, then calls
    // `inprocess_pw_add`, which sends the built `AddPendingWorkItem` command
    // straight through the mediator — it never re-enters `run_add`, so this
    // directory must hold exactly the one file written up front. The CLI
    // path that *does* land in `run_add` with `--continue-handoff --tag
    // handoff` set (an operator's `--pending-work-script` execing the real
    // `pwf` binary) is guarded separately by
    // `add_continue_handoff_and_tag_handoff_does_not_scaffold_a_second_file`.
    let handoff_files: Vec<_> = fs::read_dir(repo.join("docs/handoffs"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "md") && e.file_name() != "LEDGER.md"
        })
        .collect();
    assert_eq!(
        handoff_files.len(),
        1,
        "expected exactly one handoff file, got: {:?}",
        handoff_files
            .iter()
            .map(std::fs::DirEntry::path)
            .collect::<Vec<_>>()
    );
}

// ── Task 10 / PWF-0117: `pwf add --tag handoff` scaffolds the handoff ──────

#[test]
fn add_with_handoff_tag_scaffolds_handoff_file() {
    let stage = tmpdir("pw_add_handoff_tag");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "add",
        "test-project",
        "ship the thing / do it",
        "--tag",
        "handoff",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
        "--date",
        "2026-01-01",
    ]);
    let out = pwk::run_args(&args).unwrap();

    assert!(out.contains("TST-0001"), "got: {out}");

    let item = fs::read_to_string(notes.join("test-project/TST-0001.md")).unwrap();
    assert!(item.contains("tags: [handoff]"), "got: {item}");

    let handoff_path = repo.join("docs/handoffs/2026-01-01-ship-the-thing.md");
    let handoff = fs::read_to_string(&handoff_path).unwrap();
    assert!(handoff.contains("pw: TST-0001"), "got: {handoff}");

    let ledger = fs::read_to_string(repo.join("docs/handoffs/LEDGER.md")).unwrap();
    assert!(ledger.contains("TST-0001"), "got: {ledger}");
}

#[test]
fn add_with_handoff_tag_and_missing_repo_root_errors_without_creating_item() {
    let stage = tmpdir("pw_add_handoff_tag_missing_repo");
    // `repo` is mapped in config but never created on disk.
    let repo = stage.join("repo");
    let notes = stage.join("notes");
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "add",
        "test-project",
        "ship the thing / do it",
        "--tag",
        "handoff",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
        "--date",
        "2026-01-01",
    ]);
    let err = pwk::run_args(&args).unwrap_err();

    assert!(err.contains("does not exist"), "got: {err}");
    assert!(
        !notes.join("test-project").exists()
            || fs::read_dir(notes.join("test-project"))
                .unwrap()
                .next()
                .is_none(),
        "no item should have been created when the handoff preflight fails"
    );
}

#[test]
fn add_with_handoff_tag_and_scaffold_path_collision_errors_without_creating_item() {
    let stage = tmpdir("pw_add_handoff_tag_collision");
    let repo = stage.join("repo");
    let handoff_dir = repo.join("docs/handoffs");
    fs::create_dir_all(&handoff_dir).unwrap();
    fs::write(
        handoff_dir.join("2026-01-01-ship-the-thing.md"),
        "existing\n",
    )
    .unwrap();
    let notes = stage.join("notes");
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "add",
        "test-project",
        "ship the thing / do it",
        "--tag",
        "handoff",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
        "--date",
        "2026-01-01",
    ]);
    let err = pwk::run_args(&args).unwrap_err();

    assert!(err.contains("already exists"), "got: {err}");
    assert!(
        !notes.join("test-project").exists()
            || fs::read_dir(notes.join("test-project"))
                .unwrap()
                .next()
                .is_none(),
        "no item should have been created on a scaffold path collision"
    );
    let existing = fs::read_to_string(handoff_dir.join("2026-01-01-ship-the-thing.md")).unwrap();
    assert_eq!(existing, "existing\n", "existing handoff must be untouched");
}

#[test]
fn add_without_handoff_tag_has_no_handoff_side_effects() {
    let stage = tmpdir("pw_add_no_handoff_tag");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "add",
        "test-project",
        "ship the thing / do it",
        "--tag",
        "godot",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
        "--date",
        "2026-01-01",
    ]);
    let out = pwk::run_args(&args).unwrap();

    assert!(out.contains("TST-0001"), "got: {out}");
    let item = fs::read_to_string(notes.join("test-project/TST-0001.md")).unwrap();
    assert!(item.contains("tags: [godot]"), "got: {item}");
    assert!(
        !repo.join("docs/handoffs").exists(),
        "a plain (non-handoff) tag must not create a handoff dir"
    );
}

/// `--continue-handoff --tag handoff` is the canonical shape an operator's
/// `--pending-work-script` allocator execs against the real `pwf` binary
/// (`pw_bridge::spawn_pw_add`'s doc comment). That flag means the item
/// continues a handoff that already exists, so `run_add` must not scaffold a
/// second one — only guard proving this drives `run_add` directly; the
/// in-process `handoff add` seam (`inprocess_pw_add`) never reaches it.
#[test]
fn add_continue_handoff_and_tag_handoff_does_not_scaffold_a_second_file() {
    let stage = tmpdir("pw_add_continue_no_double_scaffold");
    let repo = stage.join("repo");
    let handoff_dir = repo.join("docs/handoffs");
    fs::create_dir_all(&handoff_dir).unwrap();
    fs::write(
        handoff_dir.join("2026-01-01-existing-handoff.md"),
        "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\n---\n\n# Existing Handoff\n",
    )
    .unwrap();
    let notes = stage.join("notes");
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "add",
        "test-project",
        "--continue-handoff",
        "--tag",
        "handoff",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
        "--date",
        "2026-01-02",
    ]);
    let out = pwk::run_args(&args).unwrap();

    assert!(out.contains("TST-0001"), "got: {out}");
    let item = fs::read_to_string(notes.join("test-project/TST-0001.md")).unwrap();
    assert!(item.contains("tags: [handoff]"), "got: {item}");

    let handoff_files: Vec<_> = fs::read_dir(&handoff_dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            e.path().extension().is_some_and(|ext| ext == "md") && e.file_name() != "LEDGER.md"
        })
        .collect();
    assert_eq!(
        handoff_files.len(),
        1,
        "--continue-handoff must not scaffold a second handoff file, got: {:?}",
        handoff_files
            .iter()
            .map(std::fs::DirEntry::path)
            .collect::<Vec<_>>()
    );
}

// ── Task 22: list dispatch ─────────────────────────────────────────────────

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

// ── Task 7 / PWF-0117: mirror `pwf done`/`pwf cancel` onto the linked handoff ──

fn parse_pw_args(tokens: &[&str]) -> pwf::cli::Args {
    let v = tokens
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    pwf::command::parse_argv(v).unwrap().1
}

/// Stage a `test-project` item tagged `handoff` plus its open index link, and
/// — when `handoff_pw` is set — a matching active handoff file under
/// `docs/handoffs/` in `repo` linking back via `pw: <handoff_pw>`.
fn stage_tagged_item(notes: &Path, repo: &Path, handoff_pw: Option<&str>) {
    let proj = notes.join("test-project");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("TST-0001.md"),
        "---\nid: TST-0001\nstatus: active\ntitle: tray gui\nproject: test-project\ncreated: 2026-01-01\ntags: [handoff]\n---\n\nbody\n",
    )
    .unwrap();
    fs::write(
        proj.join("test-project.md"),
        "---\nid: tst\ntitle: test-project\n---\n\n- [ ] [[TST-0001]]\n",
    )
    .unwrap();

    if let Some(pw) = handoff_pw {
        let handoff_dir = repo.join("docs/handoffs");
        fs::create_dir_all(&handoff_dir).unwrap();
        fs::write(
            handoff_dir.join("2026-01-01-managed-flow.md"),
            format!(
                "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: {pw}\n---\n\n# Managed Flow\n"
            ),
        )
        .unwrap();
    }
}

#[test]
fn done_archives_linked_handoff_and_reports_dest() {
    let stage = tmpdir("hf_done_mirror");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    stage_tagged_item(&notes, &repo, Some("TST-0001"));
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "done",
        "--id",
        "TST-0001",
        "--report",
        "x",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
        "--date",
        "2026-01-02",
    ]);
    let out = pwk::run_args(&args).unwrap();

    assert!(out.contains("handoff: archived"), "got: {out}");
    let archived_path = repo.join("docs/handoffs/archived/2026-01-01-managed-flow.md");
    let archived = fs::read_to_string(&archived_path).unwrap();
    assert!(archived.contains("status: done"), "got: {archived}");
    assert!(
        archived.contains("completed: 2026-01-02"),
        "got: {archived}"
    );
    assert!(
        !repo
            .join("docs/handoffs/2026-01-01-managed-flow.md")
            .exists(),
        "active handoff should have been moved"
    );

    let ledger = fs::read_to_string(repo.join("docs/handoffs/LEDGER.md")).unwrap();
    assert!(
        !ledger.contains("TST-0001"),
        "closed item should have no ledger row: {ledger}"
    );

    let item = fs::read_to_string(notes.join("test-project/TST-0001.md")).unwrap();
    assert!(item.contains("status: done"), "got: {item}");
}

#[test]
fn cancel_archives_linked_handoff_as_cancelled_with_report_body() {
    let stage = tmpdir("hf_cancel_mirror");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    stage_tagged_item(&notes, &repo, Some("TST-0001"));
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "cancel",
        "--id",
        "TST-0001",
        "--report",
        "why",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
        "--date",
        "2026-01-02",
    ]);
    let out = pwk::run_args(&args).unwrap();

    assert!(out.contains("handoff: archived"), "got: {out}");
    let archived_path = repo.join("docs/handoffs/archived/2026-01-01-managed-flow.md");
    let archived = fs::read_to_string(&archived_path).unwrap();
    assert!(archived.contains("status: cancelled"), "got: {archived}");
    assert!(
        archived.trim_end().ends_with("> Cancelled: why"),
        "got: {archived}"
    );

    let item = fs::read_to_string(notes.join("test-project/TST-0001.md")).unwrap();
    assert!(item.contains("status: cancelled"), "got: {item}");
}

#[test]
fn done_errors_and_leaves_item_active_when_no_handoff_links_it() {
    let stage = tmpdir("hf_done_mirror_missing");
    let repo = stage.join("repo");
    fs::create_dir_all(repo.join("docs/handoffs")).unwrap();
    let notes = stage.join("notes");
    stage_tagged_item(&notes, &repo, None);
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "done",
        "--id",
        "TST-0001",
        "--report",
        "x",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
        "--date",
        "2026-01-02",
    ]);
    let err = pwk::run_args(&args).unwrap_err();

    assert!(err.contains("pw: TST-0001"), "got: {err}");
    let item = fs::read_to_string(notes.join("test-project/TST-0001.md")).unwrap();
    assert!(
        item.contains("status: active"),
        "item mutated despite preflight failure: {item}"
    );
}

#[test]
fn done_on_untagged_item_never_touches_handoffs_dir() {
    let stage = tmpdir("hf_done_untagged");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    let proj = notes.join("test-project");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("TST-0001.md"),
        "---\nid: TST-0001\nstatus: active\ntitle: tray gui\nproject: test-project\ncreated: 2026-01-01\n---\n\nbody\n",
    )
    .unwrap();
    fs::write(
        proj.join("test-project.md"),
        "---\nid: tst\ntitle: test-project\n---\n\n- [ ] [[TST-0001]]\n",
    )
    .unwrap();
    let handoff_dir = repo.join("docs/handoffs");
    fs::create_dir_all(&handoff_dir).unwrap();
    fs::write(
        handoff_dir.join("2026-01-01-managed-flow.md"),
        "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n",
    )
    .unwrap();
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "done",
        "--id",
        "TST-0001",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
        "--date",
        "2026-01-02",
    ]);
    let out = pwk::run_args(&args).unwrap();

    assert!(!out.contains("handoff: archived"), "got: {out}");
    assert!(
        handoff_dir.join("2026-01-01-managed-flow.md").exists(),
        "untagged done must not touch the handoff file"
    );
    assert!(
        !handoff_dir
            .join("archived/2026-01-01-managed-flow.md")
            .exists()
    );
    let item = fs::read_to_string(proj.join("TST-0001.md")).unwrap();
    assert!(item.contains("status: done"), "got: {item}");
}

#[test]
fn done_errors_and_leaves_item_active_when_archive_destination_exists() {
    let stage = tmpdir("hf_done_mirror_conflict");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    stage_tagged_item(&notes, &repo, Some("TST-0001"));
    let archive_dir = repo.join("docs/handoffs/archived");
    fs::create_dir_all(&archive_dir).unwrap();
    fs::write(
        archive_dir.join("2026-01-01-managed-flow.md"),
        "existing archive\n",
    )
    .unwrap();
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "done",
        "--id",
        "TST-0001",
        "--report",
        "x",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
        "--date",
        "2026-01-02",
    ]);
    let err = pwk::run_args(&args).unwrap_err();

    assert!(
        err.contains("archived handoff already exists"),
        "got: {err}"
    );
    let item = fs::read_to_string(notes.join("test-project/TST-0001.md")).unwrap();
    assert!(
        item.contains("status: active"),
        "item mutated despite preflight failure: {item}"
    );
}

// ── Task 8 / PWF-0117: mirror `pwf reopen` onto the archived handoff ───────

/// Stage a closed `test-project` item tagged `handoff` plus its done-queue
/// index link, and — when `handoff_pw` is set — a matching archived handoff
/// file under `docs/handoffs/archived/` in `repo` linking back via
/// `pw: <handoff_pw>`.
fn stage_closed_tagged_item(notes: &Path, repo: &Path, handoff_pw: Option<&str>) {
    let proj = notes.join("test-project");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("TST-0001.md"),
        "---\nid: TST-0001\nstatus: done\ntitle: tray gui\nproject: test-project\ncreated: 2026-01-01\ncompleted: 2026-01-02\ntags: [handoff]\n---\n\nbody\n",
    )
    .unwrap();
    fs::write(
        proj.join("test-project.md"),
        "---\nid: tst\ntitle: test-project\n---\n\n- [x] [[TST-0001]] \u{2705} 2026-01-02\n",
    )
    .unwrap();

    if let Some(pw) = handoff_pw {
        let archive_dir = repo.join("docs/handoffs/archived");
        fs::create_dir_all(&archive_dir).unwrap();
        fs::write(
            archive_dir.join("2026-01-01-managed-flow.md"),
            format!(
                "---\nstatus: done\ncompleted: 2026-01-02\nproject: test-project\ncreated: 2026-01-01\npw: {pw}\n---\n\n# Managed Flow\n"
            ),
        )
        .unwrap();
    }
}

#[test]
fn reopen_restores_archived_handoff_and_reports_dest() {
    let stage = tmpdir("hf_reopen_mirror");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    stage_closed_tagged_item(&notes, &repo, Some("TST-0001"));
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "reopen",
        "--id",
        "TST-0001",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
    ]);
    let out = pwk::run_args(&args).unwrap();

    assert!(out.contains("handoff: reopened"), "got: {out}");
    let active_path = repo.join("docs/handoffs/2026-01-01-managed-flow.md");
    let active = fs::read_to_string(&active_path).unwrap();
    assert!(active.contains("status: active"), "got: {active}");
    assert!(!active.contains("completed:"), "got: {active}");
    assert!(
        !repo
            .join("docs/handoffs/archived/2026-01-01-managed-flow.md")
            .exists(),
        "archived handoff should have been moved"
    );

    let ledger = fs::read_to_string(repo.join("docs/handoffs/LEDGER.md")).unwrap();
    assert!(
        ledger.contains("TST-0001"),
        "reopened item should have a ledger row: {ledger}"
    );

    let item = fs::read_to_string(notes.join("test-project/TST-0001.md")).unwrap();
    assert!(item.contains("status: active"), "got: {item}");
    assert!(!item.contains("completed:"), "got: {item}");
}

#[test]
fn reopen_errors_and_leaves_item_done_when_active_destination_exists() {
    let stage = tmpdir("hf_reopen_mirror_conflict");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    stage_closed_tagged_item(&notes, &repo, Some("TST-0001"));
    let handoff_dir = repo.join("docs/handoffs");
    fs::create_dir_all(&handoff_dir).unwrap();
    fs::write(
        handoff_dir.join("2026-01-01-managed-flow.md"),
        "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0002\n---\n\n# Conflicting\n",
    )
    .unwrap();
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "reopen",
        "--id",
        "TST-0001",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
    ]);
    let err = pwk::run_args(&args).unwrap_err();

    assert!(err.contains("active handoff already exists"), "got: {err}");
    let item = fs::read_to_string(notes.join("test-project/TST-0001.md")).unwrap();
    assert!(
        item.contains("status: done"),
        "item mutated despite preflight failure: {item}"
    );
}

#[test]
fn reopen_errors_and_leaves_item_done_when_no_archived_handoff_links_it() {
    let stage = tmpdir("hf_reopen_mirror_missing");
    let repo = stage.join("repo");
    fs::create_dir_all(repo.join("docs/handoffs/archived")).unwrap();
    let notes = stage.join("notes");
    stage_closed_tagged_item(&notes, &repo, None);
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "reopen",
        "--id",
        "TST-0001",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
    ]);
    let err = pwk::run_args(&args).unwrap_err();

    assert!(err.contains("pw: TST-0001"), "got: {err}");
    let item = fs::read_to_string(notes.join("test-project/TST-0001.md")).unwrap();
    assert!(
        item.contains("status: done"),
        "item mutated despite preflight failure: {item}"
    );
}

#[test]
fn reopen_already_active_pair_skips_without_touching_handoff() {
    let stage = tmpdir("hf_reopen_active_pair");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    // Tagged ACTIVE item whose handoff is also already active: the pair is in
    // its goal state, so reopen must stay FR-0021's idempotent no-op skip.
    stage_tagged_item(&notes, &repo, Some("TST-0001"));
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "reopen",
        "--id",
        "TST-0001",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
    ]);
    let out = pwk::run_args(&args).unwrap();

    assert!(out.contains("already active"), "got: {out}");
    assert!(!out.contains("handoff: reopened"), "got: {out}");
    let handoff =
        fs::read_to_string(repo.join("docs/handoffs/2026-01-01-managed-flow.md")).unwrap();
    assert!(
        handoff.contains("status: active"),
        "active handoff mutated: {handoff}"
    );
    assert!(
        !repo
            .join("docs/handoffs/archived/2026-01-01-managed-flow.md")
            .exists(),
        "reopen of an active pair must not create an archived copy"
    );
}

#[test]
fn reopen_on_untagged_item_never_touches_handoffs_dir() {
    let stage = tmpdir("hf_reopen_untagged");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    let proj = notes.join("test-project");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("TST-0001.md"),
        "---\nid: TST-0001\nstatus: done\ntitle: tray gui\nproject: test-project\ncreated: 2026-01-01\ncompleted: 2026-01-02\n---\n\nbody\n",
    )
    .unwrap();
    fs::write(
        proj.join("test-project.md"),
        "---\nid: tst\ntitle: test-project\n---\n\n- [x] [[TST-0001]] \u{2705} 2026-01-02\n",
    )
    .unwrap();
    let archive_dir = repo.join("docs/handoffs/archived");
    fs::create_dir_all(&archive_dir).unwrap();
    fs::write(
        archive_dir.join("2026-01-01-managed-flow.md"),
        "---\nstatus: done\ncompleted: 2026-01-02\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n",
    )
    .unwrap();
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "reopen",
        "--id",
        "TST-0001",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
    ]);
    let out = pwk::run_args(&args).unwrap();

    assert!(!out.contains("handoff: reopened"), "got: {out}");
    assert!(
        archive_dir.join("2026-01-01-managed-flow.md").exists(),
        "untagged reopen must not touch the handoff file"
    );
    let item = fs::read_to_string(proj.join("TST-0001.md")).unwrap();
    assert!(item.contains("status: active"), "got: {item}");
}

// ── Task 9 / PWF-0117: mirror `pwf remove` onto the linked handoff ─────────

#[test]
fn remove_deletes_linked_handoff_and_rebuilds_ledger() {
    let stage = tmpdir("hf_remove_mirror");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    stage_tagged_item(&notes, &repo, Some("TST-0001"));
    let handoff_dir = repo.join("docs/handoffs");
    fs::write(handoff_dir.join("LEDGER.md"), "# stale\n").unwrap();
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "remove",
        "--id",
        "TST-0001",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
    ]);
    // `run_args` uses `RealConfirm`, which is non-interactive under `cargo
    // test` (no TTY on stdin), so the default-yes gate proceeds unprompted.
    let out = pwk::run_args(&args).unwrap();

    assert!(out.starts_with("REMOVED PWF TASK [TST-0001]"), "got: {out}");
    assert!(
        !notes.join("test-project/TST-0001.md").exists(),
        "note must be deleted"
    );
    assert!(
        !handoff_dir.join("2026-01-01-managed-flow.md").exists(),
        "linked handoff must be deleted"
    );
    let ledger = fs::read_to_string(handoff_dir.join("LEDGER.md")).unwrap();
    assert!(
        !ledger.contains("TST-0001"),
        "deleted item should have no ledger row: {ledger}"
    );
}

#[test]
fn remove_on_untagged_item_never_touches_handoffs_dir() {
    let stage = tmpdir("hf_remove_untagged");
    let repo = stage.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let notes = stage.join("notes");
    let proj = notes.join("test-project");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("TST-0001.md"),
        "---\nid: TST-0001\nstatus: active\ntitle: tray gui\nproject: test-project\ncreated: 2026-01-01\n---\n\nbody\n",
    )
    .unwrap();
    fs::write(
        proj.join("test-project.md"),
        "---\nid: tst\ntitle: test-project\n---\n\n- [ ] [[TST-0001]]\n",
    )
    .unwrap();
    let handoff_dir = repo.join("docs/handoffs");
    fs::create_dir_all(&handoff_dir).unwrap();
    fs::write(
        handoff_dir.join("2026-01-01-managed-flow.md"),
        "---\nstatus: active\nproject: test-project\ncreated: 2026-01-01\npw: TST-0001\n---\n\n# Managed Flow\n",
    )
    .unwrap();
    let cfg = write_config(&stage, &repo, &notes);

    let args = parse_pw_args(&[
        "remove",
        "--id",
        "TST-0001",
        "--config-path",
        cfg.to_str().unwrap(),
        "--notes-dir",
        notes.to_str().unwrap(),
    ]);
    let out = pwk::run_args(&args).unwrap();

    assert!(out.starts_with("REMOVED PWF TASK [TST-0001]"), "got: {out}");
    assert!(
        !proj.join("TST-0001.md").exists(),
        "untagged remove must still delete the note"
    );
    assert!(
        handoff_dir.join("2026-01-01-managed-flow.md").exists(),
        "untagged remove must not touch the handoff file"
    );
}
