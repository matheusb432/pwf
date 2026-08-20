#![cfg(unix)]

use std::fs;

use assert_cmd::prelude::OutputAssertExt as _;

use crate::shared::{SessionFixture, task_id, task_json};

#[test]
fn detached_dispatch_targets_the_existing_tmux_session() {
    let fixture = SessionFixture::new();

    fixture
        .database
        .command()
        .args(["session", "--id", "PWF-0001", "--agent", "claude", "--yes"])
        .env("PATH", &fixture.child_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
        .assert()
        .success();

    let invocations = fs::read_to_string(&fixture.tmux_log_path).unwrap();
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
fn missing_tmux_session_reports_the_start_command_without_mutation() {
    let fixture = SessionFixture::new();
    let project_path = fixture.directory().join("project");

    let assertion = fixture
        .database
        .command()
        .args(["session", "--id", "PWF-0001", "--agent", "claude", "--yes"])
        .env("PATH", &fixture.child_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
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
            project_path.display()
        )),
        "{stderr}"
    );
    assert_eq!(
        fs::read_to_string(&fixture.tmux_log_path).unwrap(),
        "-V\0has-session -t =pwf\0"
    );
}

#[test]
fn tmux_window_failure_reaches_the_process_exit_status() {
    let fixture = SessionFixture::new();

    fixture
        .database
        .command()
        .args(["session", "--id", "PWF-0001", "--agent", "claude", "--yes"])
        .env("PATH", &fixture.child_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
        .env("TMUX_STUB_NEW_WINDOW_EXIT_CODE", "17")
        .assert()
        .failure();

    assert!(
        fs::read_to_string(&fixture.tmux_log_path)
            .unwrap()
            .contains("new-window")
    );
}

#[test]
fn codex_naming_failure_stops_before_dispatch() {
    let fixture = SessionFixture::new();
    let app_server_log_path = fixture.directory().join("codex-app-server.jsonl");
    let resume_log_path = fixture.directory().join("codex-resume.log");

    fixture
        .database
        .command()
        .args(["session", "--id", "PWF-0001", "--agent", "codex", "--yes"])
        .env("PATH", &fixture.child_path)
        .env("CODEX_STUB_APP_SERVER_LOG", &app_server_log_path)
        .env("CODEX_STUB_NAME_ERROR", "name denied by fixture")
        .env("CODEX_STUB_RESUME_LOG", &resume_log_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
        .assert()
        .failure();

    assert!(app_server_log_path.exists());
    assert!(
        fs::read_to_string(&fixture.tmux_log_path)
            .unwrap()
            .ends_with("has-session -t =pwf\0")
    );
    assert!(!resume_log_path.exists());
}

#[test]
fn codex_dry_run_forwards_max_reasoning_effort() {
    let fixture = SessionFixture::new();
    let task_before = task_json(&fixture.database, &task_id("PWF-0001"));
    let app_server_log_path = fixture.directory().join("codex-app-server.jsonl");
    let resume_log_path = fixture.directory().join("codex-resume.log");

    let assertion = fixture
        .database
        .command()
        .args([
            "session",
            "--id",
            "pwf1",
            "--agent",
            "codex",
            "--inline",
            "--dry",
            "--effort",
            "max",
            "--push-prompt",
            "dry-run context",
        ])
        .env("PATH", &fixture.child_path)
        .env("CODEX_STUB_APP_SERVER_LOG", &app_server_log_path)
        .env("CODEX_STUB_RESUME_LOG", &resume_log_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
        .assert()
        .success();

    let stdout = String::from_utf8(assertion.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("effort: max"), "{stdout}");
    assert!(
        stdout.contains("-c 'model_reasoning_effort=\"max\"'"),
        "{stdout}"
    );
    assert!(stdout.contains("<pwf_session_context>"), "{stdout}");
    assert!(stdout.contains("dry-run context"), "{stdout}");
    assert!(stdout.contains("</pwf_session_context>"), "{stdout}");
    assert!(stdout.contains("<pwf_task>"), "{stdout}");
    assert_eq!(
        task_json(&fixture.database, &task_id("PWF-0001")),
        task_before
    );
    assert!(!app_server_log_path.exists());
    assert!(!resume_log_path.exists());
    assert!(!fixture.tmux_log_path.exists());
}

#[test]
fn direct_blocker_warning_is_emitted_for_dry_run_and_assume_yes_dispatch() {
    let fixture = SessionFixture::new();
    fixture
        .database
        .command()
        .args([
            "task",
            "add",
            "pwf",
            "--title",
            "blocking work",
            "--goal",
            "finish first",
        ])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args(["task", "edit", "PWF-0001", "--add-blocked-by", "PWF-0002"])
        .assert()
        .success();

    let dry_run = fixture
        .database
        .command()
        .args([
            "session",
            "--id",
            "PWF-0001",
            "--agent",
            "codex",
            "--inline",
            "--dry-run",
        ])
        .env("PATH", &fixture.child_path)
        .assert()
        .success();
    let dry_stderr = String::from_utf8(dry_run.get_output().stderr.clone()).unwrap();
    assert_eq!(
        dry_stderr
            .matches("warning: blocked_by information")
            .count(),
        1,
        "{dry_stderr}"
    );
    assert!(
        dry_stderr.contains("PWF-0002 (active): blocking work"),
        "{dry_stderr}"
    );

    let dispatched = fixture
        .database
        .command()
        .args(["session", "--id", "PWF-0001", "--agent", "claude", "--yes"])
        .env("PATH", &fixture.child_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
        .assert()
        .success();
    let dispatch_stderr = String::from_utf8(dispatched.get_output().stderr.clone()).unwrap();
    assert_eq!(
        dispatch_stderr
            .matches("warning: blocked_by information")
            .count(),
        1,
        "{dispatch_stderr}"
    );
    assert!(
        dispatch_stderr.contains("PWF-0002 (active): blocking work"),
        "{dispatch_stderr}"
    );
}

#[test]
fn broken_effort_tier_catalog_stops_before_dispatch() {
    let fixture = SessionFixture::new();
    fixture
        .database
        .command()
        .args(["task", "edit", "PWF-0001", "--effort", "highest"])
        .assert()
        .success();
    let missing_tiers = fixture.directory().join("does-not-exist.toml");

    fixture
        .database
        .command()
        .args(["session", "--id", "PWF-0001", "--agent", "claude", "--yes"])
        .env("PATH", &fixture.child_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
        .env("PWF_MODEL_TIERS", missing_tiers)
        .assert()
        .failure();

    assert!(
        !fixture.tmux_log_path.exists()
            || fs::read_to_string(&fixture.tmux_log_path)
                .unwrap()
                .is_empty()
    );
}

#[test]
fn inline_dispatch_executes_the_concrete_claude_process() {
    let fixture = SessionFixture::new();
    fixture.install_claude();
    let claude_log_path = fixture.directory().join("claude.log");

    fixture
        .database
        .command()
        .args([
            "session", "--id", "PWF-0001", "--agent", "claude", "--inline", "--yes",
        ])
        .env("PATH", &fixture.child_path)
        .env("CLAUDE_STUB_EXIT_CODE", "23")
        .env("CLAUDE_STUB_LOG", &claude_log_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
        .assert()
        .code(23);

    let project_path = fixture.directory().join("project");
    let log = fs::read(&claude_log_path).unwrap();
    let entries = log
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| String::from_utf8(entry.to_vec()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(entries[0], format!("cwd={}", project_path.display()));
    assert_eq!(
        entries[1..6],
        [
            "arg=--name",
            "arg=pwf1 :: do the thing",
            "arg=--effort",
            "arg=high",
            "arg=--",
        ]
    );
    assert!(entries[6].starts_with("arg=<pwf_task>\n"));
    assert!(entries[6].contains("do the thing"));
    assert!(entries[6].ends_with("\n</pwf_task>"));
    assert!(!fixture.tmux_log_path.exists());
}
