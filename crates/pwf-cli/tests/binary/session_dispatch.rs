#![cfg(unix)]

use std::fs;

use assert_cmd::prelude::OutputAssertExt as _;

use crate::support::{SessionFixture, task_id, task_json};

#[test]
fn codex_dry_run_forwards_max_reasoning_effort() {
    let fixture = SessionFixture::new().unwrap();
    let task_before = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();
    let app_server_log_path = fixture.directory().join("codex-app-server.jsonl");
    let resume_log_path = fixture.directory().join("codex-resume.log");

    let assertion = fixture
        .database
        .command()
        .args([
            "session",
            "--id",
            "foo1",
            "--agent",
            "codex",
            "--dry",
            "--effort",
            "max",
            "--push-prompt",
            "dry-run context",
        ])
        .env("PATH", &fixture.child_path)
        .env("CODEX_STUB_APP_SERVER_LOG", &app_server_log_path)
        .env("CODEX_STUB_RESUME_LOG", &resume_log_path)
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
        task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap(),
        task_before
    );
    assert!(!app_server_log_path.exists());
    assert!(!resume_log_path.exists());
}

#[test]
fn default_dispatch_forwards_the_explicit_model_to_the_concrete_claude_process() {
    let fixture = SessionFixture::new().unwrap();
    fixture.install_claude().unwrap();
    let claude_log_path = fixture.directory().join("claude.log");

    fixture
        .database
        .command()
        .args([
            "session",
            "--id",
            "FOO-0001",
            "--agent",
            "claude",
            "--yes",
            "--model",
            "manual-model",
        ])
        .env("PATH", &fixture.child_path)
        .env("CLAUDE_STUB_EXIT_CODE", "23")
        .env("CLAUDE_STUB_LOG", &claude_log_path)
        .assert()
        .code(23);

    let project_path = fixture.directory().join("project");
    let log = fs::read(&claude_log_path).unwrap();
    let entries = log
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| String::from_utf8(entry.to_vec()).unwrap())
        .collect::<Vec<_>>();
    let working_directory = std::path::Path::new(entries[0].strip_prefix("cwd=").unwrap());
    assert_eq!(
        working_directory.canonicalize().unwrap(),
        project_path.canonicalize().unwrap()
    );
    assert_eq!(
        entries[1..8],
        [
            "arg=--name",
            "arg=foo1 :: do the thing",
            "arg=--model",
            "arg=manual-model",
            "arg=--effort",
            "arg=high",
            "arg=--",
        ]
    );
    assert!(entries[8].starts_with("arg=<pwf_task>\n"));
    assert!(entries[8].contains("do the thing"));
    assert!(entries[8].ends_with("\n</pwf_task>"));
}
