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

#[test]
fn session_help_describes_tmux_and_inline_dispatch() {
    let output = database_independent_command()
        .args(["session", "--help"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("project's tmux session as a new window"),
        "{stdout}"
    );
    assert!(
        stdout.contains("current terminal instead of a tmux window"),
        "{stdout}"
    );
    assert!(!stdout.contains("zellij"), "{stdout}");
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
fn stage_session_with_tmux_stub(dir: &TempDir) -> (DatabaseFixture, String, std::path::PathBuf) {
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
        ("tmux", "tests/fixtures/tmux-stub.sh"),
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

#[test]
#[cfg(unix)]
fn verify_probes_and_previews_the_selected_provider() {
    let directory = TempDir::new().unwrap();
    let (database, path, _log) = stage_session_with_tmux_stub(&directory);

    let assertion = database
        .command()
        .args(["verify", "PWF-0001", "--agent", "codex"])
        .env("PATH", path)
        .assert()
        .success();

    let stdout = String::from_utf8(assertion.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("codex: available (codex-cli fixture 1.0)"),
        "{stdout}"
    );
    assert!(stdout.contains("command: codex resume "), "{stdout}");
    assert!(!stdout.contains("claude:"), "{stdout}");
}

#[test]
#[cfg(unix)]
fn session_dispatches_a_detached_tmux_window() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_tmux_stub(&dir);

    cfg.command()
        .args(["session", "--id", "PWF-0001", "--agent", "claude", "--yes"])
        .env("PATH", path)
        .env("TMUX_STUB_LOG", &log)
        .assert()
        .success();

    let invocations = fs::read_to_string(&log).unwrap();
    assert!(invocations.contains("-V\0"), "{invocations:?}");
    assert!(
        invocations.contains("has-session -t =pwf\0"),
        "{invocations:?}"
    );
    assert!(
        invocations.contains("new-window -d -t =pwf: -c "),
        "{invocations:?}"
    );
    assert!(
        invocations.contains(" -n PWF-0001 -- claude "),
        "{invocations:?}"
    );
    assert!(!invocations.contains("new-session"), "{invocations:?}");
    assert!(!invocations.contains("switch-client"), "{invocations:?}");
    assert!(!invocations.contains("attach-session"), "{invocations:?}");
}

#[test]
#[cfg(unix)]
fn session_missing_tmux_session_prints_a_start_command_without_mutation() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_tmux_stub(&dir);
    let repository = dir.path().join("repo");

    let assertion = cfg
        .command()
        .args(["session", "--id", "PWF-0001", "--agent", "claude", "--yes"])
        .env("PATH", path)
        .env("TMUX_STUB_LOG", &log)
        .env("TMUX_STUB_SESSION_EXISTS", "0")
        .assert()
        .failure();

    let stderr = String::from_utf8(assertion.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("tmux session 'pwf' does not exist"),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!(
            "tmux new-session -d -s pwf -c {}",
            repository.display()
        )),
        "{stderr}"
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap(),
        "-V\0has-session -t =pwf\0"
    );
}

#[test]
#[cfg(unix)]
fn session_window_rejection_exits_nonzero() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_tmux_stub(&dir);

    let assertion = cfg
        .command()
        .args(["session", "--id", "PWF-0001", "--agent", "claude", "--yes"])
        .env("PATH", path)
        .env("TMUX_STUB_LOG", &log)
        .env("TMUX_STUB_NEW_WINDOW_EXIT_CODE", "17")
        .assert()
        .failure();
    let stderr = String::from_utf8(assertion.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("Failed to open tmux window"), "{stderr}");
    assert!(fs::read_to_string(log).unwrap().contains("new-window"));
}

#[test]
#[cfg(unix)]
fn session_codex_naming_failure_stops_before_dispatch() {
    let directory = TempDir::new().unwrap();
    let (config_path, child_path, tmux_log_path) = stage_session_with_tmux_stub(&directory);
    let app_server_log_path = directory.path().join("codex-app-server.jsonl");
    let resume_log_path = directory.path().join("codex-resume.log");

    config_path
        .command()
        .args(["session", "--id", "PWF-0001", "--agent", "codex", "--yes"])
        .env("PATH", child_path)
        .env("CODEX_STUB_APP_SERVER_LOG", &app_server_log_path)
        .env("CODEX_STUB_NAME_ERROR", "name denied by fixture")
        .env("CODEX_STUB_RESUME_LOG", &resume_log_path)
        .env("TMUX_STUB_LOG", &tmux_log_path)
        .assert()
        .failure();

    assert!(
        app_server_log_path.exists(),
        "Codex app server was not invoked"
    );
    assert!(
        fs::read_to_string(&tmux_log_path)
            .unwrap()
            .ends_with("has-session -t =pwf\0"),
        "naming failure opened a tmux window: {}",
        fs::read_to_string(tmux_log_path).unwrap()
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
    let (config_path, child_path, tmux_log_path) = stage_session_with_tmux_stub(&directory);
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
        .env("TMUX_STUB_LOG", &tmux_log_path)
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
    assert!(!tmux_log_path.exists());
}

#[test]
#[cfg(unix)]
fn session_inline_executes_the_concrete_claude_process() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let (cfg, path, tmux_log) = stage_session_with_tmux_stub(&dir);
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
        .env("TMUX_STUB_LOG", &tmux_log)
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
        !tmux_log.exists(),
        "inline dispatch invoked tmux: {}",
        fs::read_to_string(tmux_log).unwrap()
    );
}

#[test]
#[cfg(unix)]
fn session_with_effort_and_broken_tiers_config_fails_before_dispatch() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_tmux_stub(&dir);
    cfg.command()
        .args(["update", "PWF-0001", "--effort", "highest"])
        .assert()
        .success();
    let missing_tiers = dir.path().join("does-not-exist.toml");

    cfg.command()
        .args(["session", "--id", "PWF-0001", "--agent", "claude", "--yes"])
        .env("PATH", path)
        .env("TMUX_STUB_LOG", &log)
        .env("PWF_MODEL_TIERS", &missing_tiers)
        .assert()
        .failure();

    assert!(!log.exists() || fs::read_to_string(&log).unwrap().is_empty());
}

#[test]
#[cfg(unix)]
fn session_append_extends_the_note_before_dispatch() {
    let dir = TempDir::new().unwrap();
    let (cfg, path, log) = stage_session_with_tmux_stub(&dir);

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
        .env("TMUX_STUB_LOG", &log)
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
    let (config_path, child_path, launch_log_path) = stage_session_with_tmux_stub(&directory);
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
        .env("TMUX_STUB_LOG", &launch_log_path)
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
    let tmux_log = fs::read_to_string(&launch_log_path).unwrap();
    assert!(
        !tmux_log.contains("new-window"),
        "declined confirmation must not dispatch: {tmux_log:?}"
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
