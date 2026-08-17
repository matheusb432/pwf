#![cfg(unix)]

use std::fs;

use assert_cmd::prelude::OutputAssertExt as _;
#[cfg(target_os = "linux")]
use expectrl::Expect;

use crate::shared::{SessionFixture, assert_failure, task_id, task_json};

#[test]
fn pushed_prompt_reaches_dispatch_in_context_order_without_mutating_the_task() {
    let fixture = SessionFixture::new();
    let task_before = task_json(&fixture.database, &task_id("PWF-0001"));

    fixture
        .database
        .command()
        .args([
            "session",
            "--id",
            "PWF-0001",
            "--agent",
            "claude",
            "--yes",
            "-p",
            "one more thing in the moment",
            "--auto",
            "--worktree",
        ])
        .env("PATH", &fixture.child_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
        .assert()
        .success();

    assert_eq!(
        task_json(&fixture.database, &task_id("PWF-0001")),
        task_before
    );

    let argv = fs::read_to_string(&fixture.tmux_log_path).unwrap();
    let context_open = argv.find("<pwf_session_context>").unwrap();
    let pushed_prompt = argv.find("one more thing in the moment").unwrap();
    let autonomy = argv.find("You MUST execute this autonomously").unwrap();
    let worktree = argv.find("git worktree here named `PWF-0001`").unwrap();
    let context_close = argv.find("</pwf_session_context>").unwrap();
    let task_open = argv.find("<pwf_task>").unwrap();
    assert!(context_open < pushed_prompt);
    assert!(pushed_prompt < autonomy);
    assert!(autonomy < worktree);
    assert!(worktree < context_close);
    assert!(context_close < task_open);
}

#[test]
#[cfg(target_os = "linux")]
fn declined_pushed_prompt_leaves_the_task_unchanged_and_does_not_dispatch() {
    let fixture = SessionFixture::new();
    let task_before = task_json(&fixture.database, &task_id("PWF-0001"));

    let mut command = fixture.database.command();
    command
        .args([
            "session",
            "--id",
            "PWF-0001",
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
        task_json(&fixture.database, &task_id("PWF-0001")),
        task_before
    );
    let tmux_log = fs::read_to_string(&fixture.tmux_log_path).unwrap();
    assert!(!tmux_log.contains("new-window"));
}

#[test]
fn session_requires_yes_without_a_terminal_and_does_not_dispatch() {
    let fixture = SessionFixture::new();
    let task_before = task_json(&fixture.database, &task_id("PWF-0001"));

    let output = fixture
        .database
        .command()
        .args(["session", "PWF-0001", "--agent", "claude"])
        .env("PATH", &fixture.child_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
        .output()
        .unwrap();

    assert_failure(
        output,
        &["interactive confirmation requires a terminal", "--yes"],
    );
    assert_eq!(
        task_json(&fixture.database, &task_id("PWF-0001")),
        task_before
    );
    let tmux_log = fs::read_to_string(&fixture.tmux_log_path).unwrap_or_default();
    assert!(!tmux_log.contains("new-window"));
}

#[test]
fn session_help_exposes_only_the_ephemeral_prompt_prefix() {
    let fixture = SessionFixture::new();

    let assertion = fixture
        .database
        .command()
        .args(["session", "--help"])
        .assert()
        .success();
    let stdout = String::from_utf8(assertion.get_output().stdout.clone()).unwrap();

    assert!(stdout.contains("-p, --push-prompt <TEXT>"), "{stdout}");
    assert!(
        stdout.contains("Prefix text pushed to the agent prompt"),
        "{stdout}"
    );
    assert!(!stdout.contains("--append"), "{stdout}");
}

#[test]
fn invalid_or_retired_prompt_flags_fail_before_session_effects() {
    let fixture = SessionFixture::new();
    let oversized = "x".repeat(2001);
    let cases = [
        vec!["session", "PWF-0001", "--push-prompt", " \t "],
        vec![
            "session",
            "PWF-0001",
            "--push-prompt",
            "first",
            "--push-prompt",
            "second",
        ],
        vec!["session", "PWF-0001", "--append", "retired"],
        vec!["session", "PWF-0001", "-a", "retired"],
    ];

    for arguments in cases {
        fixture
            .database
            .command()
            .args(arguments)
            .assert()
            .failure();
    }
    fixture
        .database
        .command()
        .args(["session", "PWF-0001", "--push-prompt", &oversized])
        .assert()
        .failure();

    assert!(!fixture.tmux_log_path.exists());
}
