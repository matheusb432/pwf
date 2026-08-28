#![cfg(unix)]

use std::fs;

use assert_cmd::prelude::OutputAssertExt as _;
#[cfg(target_os = "linux")]
use expectrl::Expect;

use crate::support::{SessionFixture, assert_failure, command, task_id, task_json};

#[test]
#[cfg(target_os = "linux")]
fn declined_pushed_prompt_leaves_the_task_unchanged_and_does_not_dispatch() {
    let fixture = SessionFixture::new().unwrap();
    let task_before = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();

    let mut command = fixture.database.command();
    command
        .args([
            "session",
            "--id",
            "FOO-0001",
            "--agent",
            "claude",
            "--effort",
            "xhigh",
            "--push-prompt",
            "declined context",
        ])
        .env("PATH", &fixture.child_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
        .env("NO_COLOR", "1");

    let mut session = expectrl::Session::spawn(command).unwrap();
    session.set_expect_timeout(Some(std::time::Duration::from_secs(10)));
    session.expect("Effort").unwrap();
    session.expect("xhigh").unwrap();
    session.expect("Prompt prefix").unwrap();
    session.expect("yes").unwrap();
    session.expect("(y/n)").unwrap();
    session.expect("yes").unwrap();
    session.send("n").unwrap();
    session.expect(expectrl::Eof).unwrap();
    assert!(matches!(
        session.get_process().wait().unwrap(),
        expectrl::process::unix::WaitStatus::Exited(_, 0)
    ));

    assert_eq!(
        task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap(),
        task_before
    );
    let tmux_log = fs::read_to_string(&fixture.tmux_log_path).unwrap();
    assert!(!tmux_log.contains("new-window"));
}

#[test]
fn session_requires_yes_without_a_terminal_and_does_not_dispatch() {
    let fixture = SessionFixture::new().unwrap();
    let task_before = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();

    let output = fixture
        .database
        .command()
        .args(["session", "FOO-0001", "--agent", "claude"])
        .env("PATH", &fixture.child_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
        .output()
        .unwrap();

    assert_failure(
        output,
        &["interactive confirmation requires a terminal", "--yes"],
    )
    .unwrap();
    assert_eq!(
        task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap(),
        task_before
    );
    let tmux_log = fs::read_to_string(&fixture.tmux_log_path).unwrap_or_default();
    assert!(!tmux_log.contains("new-window"));
}

#[test]
fn session_help_exposes_only_the_ephemeral_prompt_prefix() {
    let assertion = command().args(["session", "--help"]).assert().success();
    let stdout = String::from_utf8(assertion.get_output().stdout.clone()).unwrap();

    assert!(stdout.contains("-p, --push-prompt <TEXT>"), "{stdout}");
    assert!(
        stdout.contains("Prefix text pushed to the agent prompt"),
        "{stdout}"
    );
}
