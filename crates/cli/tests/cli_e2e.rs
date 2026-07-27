//! Exercises end-to-end behavior through the built binary and local process fixtures.

use std::{fs, process::Command};

use assert_cmd::prelude::OutputAssertExt as _;
#[cfg(target_os = "linux")]
use expectrl::Expect;
use serde_json::{Value, json};
use tempfile::TempDir;

#[path = "support/database.rs"]
mod database;

use database::DatabaseFixture;

fn database_independent_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pwf"))
}

fn managed_project(id: &str, title: &str) -> (TempDir, DatabaseFixture) {
    let directory = TempDir::new().unwrap();
    let tasks_path = directory.path().join("notes").join(title);
    let repository = directory.path().join("repo");
    fs::create_dir_all(&tasks_path).unwrap();
    fs::create_dir_all(&repository).unwrap();
    fs::write(
        tasks_path.join(format!("{title}.md")),
        format!(
            "---\nid: {}\ntitle: {title}\n---\n",
            id.to_ascii_lowercase()
        ),
    )
    .unwrap();
    finish_fixture(
        directory,
        &[ProjectSeed {
            id,
            title,
            repository: &repository,
            tasks_path: &tasks_path,
        }],
    )
}

fn task_json(database: &DatabaseFixture, id: &str) -> Value {
    let output = database
        .command()
        .args(["show", id, "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "show {id} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("show stdout is JSON")
}

struct ProjectSeed<'fixture> {
    id: &'fixture str,
    title: &'fixture str,
    repository: &'fixture std::path::Path,
    tasks_path: &'fixture std::path::Path,
}

fn finish_fixture(dir: TempDir, projects: &[ProjectSeed<'_>]) -> (TempDir, DatabaseFixture) {
    let database = DatabaseFixture::new(dir.path().join("projects.sqlite3"));
    for project in projects {
        database.add_directory_project(
            project.id,
            project.title,
            project.repository,
            project.tasks_path,
        );
    }
    (dir, database)
}

/// Stages a fresh repository without Git metadata for the handoff lifecycle round trip.
fn staged_for_handoff_mirror_roundtrip() -> (TempDir, DatabaseFixture) {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let proj = notes.join("foo-bar");
    let repo = dir.path().join("repo");
    fs::create_dir_all(&proj).unwrap();
    fs::create_dir_all(repo.join("docs").join("handoffs")).unwrap();
    fs::write(
        proj.join("foo-bar.md"),
        "---\nid: foo\ntitle: foo-bar\n---\n",
    )
    .unwrap();
    finish_fixture(
        dir,
        &[ProjectSeed {
            id: "FOO",
            title: "foo-bar",
            repository: &repo,
            tasks_path: &proj,
        }],
    )
}

/// Stages the exact `test-project` identity used by the external allocator protocol.
fn staged_for_handoff_allocator_contract() -> (TempDir, DatabaseFixture) {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let project = notes.join("test-project");
    let repo = dir.path().join("repo");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        project.join("test-project.md"),
        "---\nid: tst\ntitle: test-project\n---\n",
    )
    .unwrap();
    finish_fixture(
        dir,
        &[ProjectSeed {
            id: "TST",
            title: "test-project",
            repository: &repo,
            tasks_path: &project,
        }],
    )
}

fn add_linked_handoff(stage: &TempDir, database: &DatabaseFixture) -> std::path::PathBuf {
    let handoff_path = stage
        .path()
        .join("repo/docs/handoffs/2026-01-01-mirror-round-trip.md");
    database
        .command()
        .args([
            "add",
            "foo-bar",
            "mirror round trip",
            "--tag",
            "handoff",
            "--title",
            "mirror round trip",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .success();
    assert!(handoff_path.exists(), "linked handoff was not created");
    handoff_path
}

fn handoff_add_command(stage: &TempDir, database: &DatabaseFixture) -> Command {
    let mut command = database.command();
    command
        .args([
            "handoff",
            "add",
            "--title",
            "Managed Flow",
            "--slug",
            "managed-flow",
            "--repo-root",
        ])
        .arg(stage.path().join("repo"))
        .args(["--date", "2026-01-01"]);
    command
}

#[test]
fn add_human_flag_rejects_unreadable_index_before_mutation() {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let proj = notes.join("foo-bar");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("FOO-0001.md"),
        "---\nid: FOO-0001\nstatus: active\ntitle: tray gui\nproject: foo-bar\ncreated: 2026-01-01\n---\n\nadd toggle\n",
    )
    .unwrap();
    fs::create_dir_all(proj.join("foo-bar.md")).unwrap();
    let database = DatabaseFixture::new(dir.path().join("projects.sqlite3"));
    database.add_directory_project("FOO", "foo-bar", std::path::Path::new("/repo"), &proj);

    let output = database
        .command()
        .args(["add", "foo-bar", "x", "--human"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "{output:?}");
    database
        .command()
        .args(["show", "FOO-0002", "--json"])
        .assert()
        .failure();
}

#[test]
fn list_fails_when_an_item_cannot_be_read() {
    let (directory, database) = managed_project("FOO", "foo-bar");
    database
        .command()
        .args([
            "add",
            "foo-bar",
            "unreadable work",
            "--title",
            "unreadable",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .success();
    let item = directory.path().join("notes/foo-bar/FOO-0001.md");
    fs::remove_file(&item).unwrap();
    fs::create_dir(&item).unwrap();

    database.command().args(["list"]).assert().failure();
}

#[test]
fn add_handoff_failure_keeps_the_task_and_removes_the_scaffold() {
    let (stage, database) = staged_for_handoff_mirror_roundtrip();
    let handoff_directory = stage.path().join("repo/docs/handoffs");
    fs::create_dir(handoff_directory.join("LEDGER.md")).unwrap();

    database
        .command()
        .args([
            "add",
            "foo-bar",
            "ship the thing",
            "--title",
            "Ship: Thing",
            "--tag",
            "handoff",
            "--human",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .failure();

    let task = task_json(&database, "FOO-0001");
    assert_eq!(task["status"], "active");
    assert_eq!(task["tags"], json!(["handoff"]));
    assert_eq!(task["section"], "Human");
    assert!(
        !handoff_directory.join("2026-01-01-ship-thing.md").exists(),
        "failed ledger write must remove the new scaffold"
    );
}

#[test]
fn pending_work_list_and_review_behaviors_compose() {
    let (_directory, database) = managed_project("FOO", "foo-bar");
    database
        .command()
        .args([
            "add",
            "foo-bar",
            "implementation work",
            "--title",
            "implementation",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .success();
    database
        .command()
        .args([
            "done",
            "FOO-0001",
            "--commits",
            "a..b",
            "--review",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .success();
    database
        .command()
        .args([
            "add",
            "foo-bar",
            "cancelled work",
            "--title",
            "cancelled",
            "--date",
            "2026-01-02",
        ])
        .assert()
        .success();
    database
        .command()
        .args([
            "cancel",
            "FOO-0003",
            "--report",
            "superseded",
            "--date",
            "2026-01-03",
        ])
        .assert()
        .success();

    let review = task_json(&database, "FOO-0002");
    assert_eq!(review["status"], "active");
    assert_eq!(review["section"], "Human");
    assert!(
        review["prompt"]
            .as_str()
            .is_some_and(|prompt| prompt.contains("FOO-0001") && prompt.contains("a..b"))
    );

    for (status, present, absent) in [
        ("active", "FOO-0002", ["FOO-0001", "FOO-0003"]),
        ("done", "FOO-0001", ["FOO-0002", "FOO-0003"]),
        ("cancelled", "FOO-0003", ["FOO-0001", "FOO-0002"]),
    ] {
        let output = database
            .command()
            .args(["list", "--status", status, "--all"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains(present), "{status} list: {stdout}");
        for id in absent {
            assert!(!stdout.contains(id), "{status} list leaked {id}: {stdout}");
        }
    }
}

#[test]
fn pending_work_lifecycle_is_observable_through_show_json() {
    let (_directory, database) = managed_project("FOO", "foo-bar");
    database
        .command()
        .args([
            "add",
            "foo-bar",
            "prerequisite work",
            "--title",
            "prerequisite",
            "--date",
            "2026-06-19",
        ])
        .assert()
        .success();
    database
        .command()
        .args([
            "add",
            "foo-bar",
            "finish it",
            "--title",
            "just done",
            "--date",
            "2026-06-20",
            "--section",
            "future",
            "--prereq",
            "FOO-0001",
            "--effort",
            "high",
            "--tag",
            "cli",
            "--tag",
            "sqlite",
        ])
        .assert()
        .success();

    let active = task_json(&database, "FOO-0002");
    assert_eq!(active["id"], "FOO-0002");
    assert_eq!(active["project"], "foo-bar");
    assert_eq!(active["title"], "just done");
    assert_eq!(active["status"], "active");
    assert_eq!(active["created"], "2026-06-20");
    assert_eq!(active["tags"], json!(["cli", "sqlite"]));
    assert_eq!(active["effort"], "high");
    assert_eq!(active["prerequisites"], json!(["FOO-0001"]));
    assert_eq!(active["section"], "Future");
    assert!(
        active["prompt"]
            .as_str()
            .is_some_and(|prompt| prompt.contains("finish it"))
    );

    database
        .command()
        .args([
            "update",
            "FOO-0002",
            "--title",
            "ship it",
            "--prompt",
            "revised work",
            "--tag",
            "rust",
            "--clear-prereq",
        ])
        .assert()
        .success();
    let updated = task_json(&database, "FOO-0002");
    assert_eq!(updated["title"], "ship it");
    assert_eq!(updated["tags"], json!(["cli", "sqlite", "rust"]));
    assert_eq!(updated["prerequisites"], Value::Null);
    assert!(
        updated["prompt"]
            .as_str()
            .is_some_and(|prompt| prompt.contains("revised work"))
    );

    database
        .command()
        .args([
            "done",
            "FOO-0002",
            "--date",
            "2026-06-21",
            "--commits",
            "a..b",
        ])
        .assert()
        .success();
    let done = task_json(&database, "FOO-0002");
    assert_eq!(done["status"], "done");
    assert_eq!(done["completed"], "2026-06-21");
    assert_eq!(done["commits"], "a..b");

    database
        .command()
        .args(["reopen", "FOO-0002"])
        .assert()
        .success();
    let reopened = task_json(&database, "FOO-0002");
    assert_eq!(reopened["status"], "active");
    assert_eq!(reopened["completed"], Value::Null);
    assert_eq!(reopened["commits"], Value::Null);
    assert_eq!(reopened["prerequisites"], Value::Null);
}

#[cfg(unix)]
fn stage_session_with_zellij_stub(dir: &TempDir) -> (DatabaseFixture, String, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let notes = dir.path().join("notes");
    let proj = notes.join("pwf");
    let repo = dir.path().join("repo");
    fs::create_dir_all(&proj).unwrap();
    fs::create_dir_all(&repo).unwrap();
    fs::write(proj.join("pwf.md"), "---\nid: pwf\ntitle: pwf\n---\n").unwrap();
    let database = DatabaseFixture::new(dir.path().join("projects.sqlite3"));
    database.add_directory_project("PWF", "pwf", &repo, &proj);
    database
        .command()
        .args([
            "add",
            "pwf",
            "do the thing",
            "--title",
            "do the thing",
            "--date",
            "2026-06-20",
        ])
        .assert()
        .success();

    let bin = dir.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    for (name, fixture) in [
        ("zellij", "tests/fixtures/zellij-stub.sh"),
        ("codex", "tests/fixtures/codex-stub.sh"),
    ] {
        let stub = bin.join(name);
        fs::copy(fixture, &stub).unwrap();
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let log = dir.path().join("argv.log");
    let path = format!(
        "{}:{}",
        bin.to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    (database, path, log)
}

#[cfg(unix)]
fn zellij_command_sequence(log: &str) -> Vec<&str> {
    log.split('\0')
        .filter(|invocation| !invocation.is_empty())
        .map(|invocation| {
            if invocation.starts_with("--session ") && invocation.contains(" action new-tab ") {
                "new-tab"
            } else if invocation.starts_with("attach --create-background ") {
                "attach --create-background"
            } else {
                invocation
            }
        })
        .collect()
}

#[test]
#[cfg(unix)]
fn session_tab_rejection_exits_nonzero() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);

    cfg.command()
        .args(["session", "--id", "PWF-0001", "--agent", "claude", "--yes"])
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .env("ZELLIJ_STUB_EXIT_CODE", "17")
        .env("ZELLIJ_STUB_STDERR", "tab rejected by fixture")
        .assert()
        .failure();
    assert!(fs::read_to_string(log).unwrap().contains("new-tab"));
}

#[test]
#[cfg(unix)]
fn session_codex_naming_failure_stops_before_dispatch() {
    let directory = TempDir::new().unwrap();
    let (config_path, child_path, zellij_log_path) = stage_session_with_zellij_stub(&directory);
    let app_server_log_path = directory.path().join("codex-app-server.jsonl");
    let resume_log_path = directory.path().join("codex-resume.log");

    config_path
        .command()
        .args(["session", "--id", "PWF-0001", "--agent", "codex", "--yes"])
        .env("PATH", child_path)
        .env("CODEX_STUB_APP_SERVER_LOG", &app_server_log_path)
        .env("CODEX_STUB_NAME_ERROR", "name denied by fixture")
        .env("CODEX_STUB_RESUME_LOG", &resume_log_path)
        .env("ZELLIJ_STUB_LOG", &zellij_log_path)
        .assert()
        .failure();

    assert!(
        app_server_log_path.exists(),
        "Codex app server was not invoked"
    );
    assert!(
        !zellij_log_path.exists(),
        "naming failure invoked zellij: {}",
        fs::read_to_string(zellij_log_path).unwrap()
    );
    assert!(
        !resume_log_path.exists(),
        "naming failure invoked codex resume"
    );
}

#[test]
#[cfg(unix)]
fn session_codex_dry_run_renders_effort_without_process_effects() {
    let directory = TempDir::new().unwrap();
    let (config_path, child_path, zellij_log_path) = stage_session_with_zellij_stub(&directory);
    let app_server_log_path = directory.path().join("codex-app-server.jsonl");
    let resume_log_path = directory.path().join("codex-resume.log");

    let assertion = config_path
        .command()
        .args([
            "session", "--id", "PWF-0001", "--agent", "codex", "--inline", "--dry", "--effort",
            "xhigh",
        ])
        .env("PATH", child_path)
        .env("CODEX_STUB_APP_SERVER_LOG", &app_server_log_path)
        .env("CODEX_STUB_RESUME_LOG", &resume_log_path)
        .env("ZELLIJ_STUB_LOG", &zellij_log_path)
        .assert()
        .success();

    let stdout = String::from_utf8(assertion.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("effort: xhigh"), "{stdout}");
    assert!(
        stdout.contains("-c 'model_reasoning_effort=\"xhigh\"'"),
        "{stdout}"
    );
    assert!(stdout.contains("model: default"), "{stdout}");
    assert!(!stdout.contains("--model default"), "{stdout}");
    assert!(!app_server_log_path.exists());
    assert!(!resume_log_path.exists());
    assert!(!zellij_log_path.exists());
}

#[test]
#[cfg(unix)]
fn session_recovers_a_missing_zellij_session_once() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);
    let state = dir.path().join("zellij-state");

    cfg.command()
        .args(["session", "--id", "PWF-0001", "--agent", "claude", "--yes"])
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .env("ZELLIJ_STUB_MISSING_SESSION_COUNT", "1")
        .env("ZELLIJ_STUB_STATE", &state)
        .assert()
        .success();

    let invocations = fs::read_to_string(&log).unwrap();
    assert_eq!(
        zellij_command_sequence(&invocations),
        ["new-tab", "attach --create-background", "new-tab"]
    );
}

#[test]
#[cfg(unix)]
fn session_stops_after_a_second_missing_zellij_session() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);
    let state = dir.path().join("zellij-state");

    cfg.command()
        .args(["session", "--id", "PWF-0001", "--agent", "claude", "--yes"])
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .env("ZELLIJ_STUB_MISSING_SESSION_COUNT", "2")
        .env("ZELLIJ_STUB_STATE", &state)
        .assert()
        .failure();

    let invocations = fs::read_to_string(&log).unwrap();
    assert_eq!(
        zellij_command_sequence(&invocations),
        ["new-tab", "attach --create-background", "new-tab"]
    );
}

#[test]
#[cfg(unix)]
fn session_inline_executes_the_concrete_claude_process() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let (cfg, path, zellij_log) = stage_session_with_zellij_stub(&dir);
    let claude = dir.path().join("bin/claude");
    fs::copy("tests/fixtures/claude-stub.sh", &claude).unwrap();
    fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).unwrap();
    let claude_log = dir.path().join("claude.log");

    cfg.command()
        .args([
            "session", "--id", "PWF-0001", "--agent", "claude", "--inline", "--yes",
        ])
        .env("PATH", path)
        .env("CLAUDE_STUB_EXIT_CODE", "23")
        .env("CLAUDE_STUB_LOG", &claude_log)
        .env("ZELLIJ_STUB_LOG", &zellij_log)
        .env("ZELLIJ_STUB_LOG_VERSION", "1")
        .assert()
        .code(23);

    let repository = dir.path().join("repo");
    let log = fs::read(&claude_log).unwrap();
    let entries = log
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| String::from_utf8(entry.to_vec()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(entries[0], format!("cwd={}", repository.display()));
    assert_eq!(
        entries[1..6],
        [
            "arg=--name",
            "arg=PWF-0001 - do the thing",
            "arg=--effort",
            "arg=high",
            "arg=--",
        ]
    );
    assert!(
        entries[6].contains("do the thing"),
        "task prompt missing from Claude argv: {}",
        entries[6]
    );
    assert!(
        !zellij_log.exists(),
        "inline dispatch invoked zellij: {}",
        fs::read_to_string(zellij_log).unwrap()
    );
}

#[test]
#[cfg(unix)]
fn session_with_effort_and_broken_tiers_config_fails_before_dispatch() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);
    cfg.command()
        .args(["update", "PWF-0001", "--effort", "highest"])
        .assert()
        .success();
    let missing_tiers = dir.path().join("does-not-exist.toml");

    cfg.command()
        .args(["session", "--id", "PWF-0001", "--agent", "claude", "--yes"])
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .env("PWF_MODEL_TIERS", &missing_tiers)
        .assert()
        .failure();

    assert!(!log.exists() || fs::read_to_string(&log).unwrap().is_empty());
}

#[test]
#[cfg(unix)]
fn session_append_extends_the_note_before_dispatch() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_zellij_stub(&dir);

    cfg.command()
        .args([
            "session",
            "--id",
            "PWF-0001",
            "--yes",
            "--append",
            "one more thing in the moment",
        ])
        .env("PATH", path)
        .env("ZELLIJ_STUB_LOG", &log)
        .assert()
        .success();

    let task = task_json(&cfg, "PWF-0001");
    let prompt = task["prompt"].as_str().expect("task prompt");
    assert!(
        prompt.contains("one more thing in the moment"),
        "append did not extend the task prompt: {prompt}"
    );

    let argv = fs::read_to_string(&log).unwrap();
    assert!(
        argv.contains("one more thing in the moment"),
        "dispatched prompt did not contain the appended content: {argv}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn session_append_declined_through_stdin_leaves_note_unchanged() {
    let directory = TempDir::new().unwrap();
    let (config_path, child_path, launch_log_path) = stage_session_with_zellij_stub(&directory);
    let task_before = task_json(&config_path, "PWF-0001");

    let mut command = config_path.command();
    command
        .args([
            "session",
            "--id",
            "PWF-0001",
            "--effort",
            "xhigh",
            "--append",
            "declined context",
        ])
        .env("PATH", child_path)
        .env("ZELLIJ_STUB_LOG", &launch_log_path)
        .env("NO_COLOR", "1");

    let mut session = expectrl::Session::spawn(command).unwrap();
    session.set_expect_timeout(Some(std::time::Duration::from_secs(10)));
    session.expect("effort: xhigh").unwrap();
    session.expect("[Y/n]").unwrap();
    session.send_line("n").unwrap();
    session.expect(expectrl::Eof).unwrap();
    assert!(matches!(
        session.get_process().wait().unwrap(),
        expectrl::process::unix::WaitStatus::Exited(_, 0)
    ));

    assert_eq!(task_json(&config_path, "PWF-0001"), task_before);
    assert!(
        !launch_log_path.exists() || fs::read_to_string(&launch_log_path).unwrap().is_empty(),
        "declined confirmation must not dispatch"
    );
}

#[test]
fn note_lifecycle_preserves_pending_work_behavior() {
    let (_directory, database) = managed_project("PWF", "pwf");
    database
        .command()
        .args([
            "add",
            "pwf",
            "real task",
            "--title",
            "real task",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .success();
    let task_before = task_json(&database, "PWF-0001");

    database
        .command()
        .args(["note", "add", "pwf", "remember the milk"])
        .assert()
        .success();

    database
        .command()
        .args(["note", "update", "PWF", "1", "remember oat milk"])
        .assert()
        .success();

    let listed = database.command().args(["note", "pwf"]).output().unwrap();
    assert!(listed.status.success());
    assert!(
        String::from_utf8(listed.stdout)
            .unwrap()
            .contains("remember oat milk")
    );

    database
        .command()
        .args(["note", "remove", "pwf", "1"])
        .assert()
        .success();

    let listed = database.command().args(["note", "pwf"]).output().unwrap();
    assert!(listed.status.success());
    assert!(
        !String::from_utf8(listed.stdout)
            .unwrap()
            .contains("remember oat milk")
    );
    assert_eq!(task_json(&database, "PWF-0001"), task_before);
}

#[test]
fn handoff_list_unreadable_ledger_is_not_reported_as_missing() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().join("repo");
    fs::create_dir_all(repo.join("docs/handoffs/LEDGER.md")).unwrap();

    database_independent_command()
        .args(["handoff", "list", "--repo-root"])
        .arg(&repo)
        .assert()
        .failure();
}

#[cfg(unix)]
#[test]
fn handoff_add_external_allocator_receives_canonical_arguments() {
    let (dir, cfg) = staged_for_handoff_allocator_contract();
    let repo = dir.path().join("repo");
    let handoff = repo.join("docs/handoffs/2026-01-01-managed-flow.md");
    let allocator =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pw-stub.sh");
    let argument_log = dir.path().join("allocator-argv.txt");
    let database_path_log = dir.path().join("allocator-database-path.txt");

    handoff_add_command(&dir, &cfg)
        .arg("--pending-work-script")
        .arg(&allocator)
        .env("HANDOFF_STUB_LOG", &argument_log)
        .env("HANDOFF_STUB_DATABASE_LOG", &database_path_log)
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(&argument_log).unwrap(),
        "add\n--date\n2026-01-01\ntest-project\n--tag\nhandoff\n--continue-handoff\n"
    );
    assert_eq!(
        fs::read(&database_path_log).unwrap(),
        dir.path()
            .join("projects.sqlite3")
            .as_os_str()
            .as_encoded_bytes()
    );
    assert!(
        fs::read_to_string(&handoff)
            .unwrap()
            .contains("pw: TST-0001")
    );
}

#[cfg(unix)]
#[test]
fn handoff_add_rejects_raw_invalid_utf8_and_removes_the_provisional_document() {
    let (dir, cfg) = staged_for_handoff_allocator_contract();
    let handoff = dir
        .path()
        .join("repo/docs/handoffs/2026-01-01-managed-flow.md");
    let allocator = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pw-stub-invalid-utf8.sh");

    handoff_add_command(&dir, &cfg)
        .arg("--pending-work-script")
        .arg(&allocator)
        .assert()
        .failure();

    assert!(
        !handoff.exists(),
        "invalid allocator output must remove the provisional handoff"
    );
}

#[test]
fn linked_handoff_close_failure_reports_post_mutation_recovery() {
    let (dir, cfg) = staged_for_handoff_mirror_roundtrip();
    let handoff = add_linked_handoff(&dir, &cfg);
    let ledger = handoff.parent().unwrap().join("LEDGER.md");
    fs::remove_file(&ledger).unwrap();
    fs::create_dir(&ledger).unwrap();

    cfg.command()
        .args([
            "done",
            "FOO-0001",
            "--report",
            "complete",
            "--date",
            "2026-01-02",
        ])
        .assert()
        .failure();
    assert_eq!(task_json(&cfg, "FOO-0001")["status"], "done");
    assert!(
        handoff.exists(),
        "failed archive must leave the handoff active"
    );
}

#[test]
fn linked_handoff_remove_failure_reports_deleted_note_recovery() {
    let (dir, cfg) = staged_for_handoff_mirror_roundtrip();
    let handoff = add_linked_handoff(&dir, &cfg);
    let ledger = handoff.parent().unwrap().join("LEDGER.md");
    fs::remove_file(&ledger).unwrap();
    fs::create_dir(&ledger).unwrap();

    cfg.command()
        .args(["remove", "FOO-0001", "--yes"])
        .assert()
        .failure();
    cfg.command()
        .args(["show", "FOO-0001", "--json"])
        .assert()
        .failure();
    assert!(
        handoff.exists(),
        "failed cleanup must leave the handoff active"
    );
}

#[test]
fn linked_handoff_lifecycle_outputs_never_touch_git() {
    // The fixture omits `.git`; the complete handoff lifecycle must neither require nor create it.
    let (d, cfg) = staged_for_handoff_mirror_roundtrip();
    let repo = d.path().join("repo");
    let handoff_dir = repo.join("docs/handoffs");
    assert!(
        !repo.join(".git").exists(),
        "fixture must start without .git"
    );

    let handoff_path = add_linked_handoff(&d, &cfg);
    assert_eq!(task_json(&cfg, "FOO-0001")["status"], "active");
    assert!(
        !repo.join(".git").exists(),
        "add must not create/touch .git"
    );
    let archived_path = handoff_dir.join("archived").join(
        handoff_path
            .file_name()
            .expect("handoff path must have a file name"),
    );

    cfg.command()
        .args([
            "done",
            "--id",
            "FOO-0001",
            "--report",
            "x",
            "--commits",
            "a..b",
            "--date",
            "2026-01-02",
        ])
        .assert()
        .success();
    assert!(
        !handoff_path.exists(),
        "handoff should be moved out of the active dir once done"
    );
    assert!(archived_path.exists());
    assert_eq!(task_json(&cfg, "FOO-0001")["status"], "done");
    assert!(
        !repo.join(".git").exists(),
        "done must not create/touch .git"
    );

    cfg.command()
        .args(["reopen", "--id", "FOO-0001"])
        .assert()
        .success();
    assert!(
        !archived_path.exists(),
        "reopen should move the handoff back out of archived/"
    );
    assert!(handoff_path.exists());
    assert_eq!(task_json(&cfg, "FOO-0001")["status"], "active");
    assert!(
        !repo.join(".git").exists(),
        "reopen must not create/touch .git"
    );

    cfg.command()
        .args([
            "cancel",
            "FOO-0001",
            "--report",
            "superseded",
            "--date",
            "2026-01-03",
        ])
        .assert()
        .success();
    assert!(archived_path.exists());
    assert_eq!(task_json(&cfg, "FOO-0001")["status"], "cancelled");

    cfg.command()
        .args(["reopen", "FOO-0001"])
        .assert()
        .success();
    cfg.command()
        .args(["remove", "FOO-0001", "--yes"])
        .env("NO_COLOR", "1")
        .assert()
        .success();
    cfg.command()
        .args(["show", "FOO-0001", "--json"])
        .assert()
        .failure();
    assert!(!handoff_path.exists(), "remove left the handoff active");
    assert!(
        !repo.join(".git").exists(),
        "remove must not create/touch .git"
    );
}
