#![cfg(unix)]

use std::fs;

use assert_cmd::prelude::OutputAssertExt as _;

use crate::common::SessionFixture;

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
    let repository = fixture.directory().join("repo");

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
            repository.display()
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
fn codex_dry_run_has_no_process_effects() {
    let fixture = SessionFixture::new();
    let app_server_log_path = fixture.directory().join("codex-app-server.jsonl");
    let resume_log_path = fixture.directory().join("codex-resume.log");

    let assertion = fixture
        .database
        .command()
        .args([
            "session", "--id", "PWF-0001", "--agent", "codex", "--inline", "--dry", "--effort",
            "xhigh",
        ])
        .env("PATH", &fixture.child_path)
        .env("CODEX_STUB_APP_SERVER_LOG", &app_server_log_path)
        .env("CODEX_STUB_RESUME_LOG", &resume_log_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
        .assert()
        .success();

    let stdout = String::from_utf8(assertion.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("effort: xhigh"), "{stdout}");
    assert!(
        stdout.contains("-c 'model_reasoning_effort=\"xhigh\"'"),
        "{stdout}"
    );
    assert!(!app_server_log_path.exists());
    assert!(!resume_log_path.exists());
    assert!(!fixture.tmux_log_path.exists());
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

    let repository = fixture.directory().join("repo");
    let log = fs::read(&claude_log_path).unwrap();
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
    assert!(entries[6].contains("do the thing"));
    assert!(!fixture.tmux_log_path.exists());
}
