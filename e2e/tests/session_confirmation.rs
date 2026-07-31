#![cfg(unix)]

use std::fs;

use assert_cmd::prelude::OutputAssertExt as _;
#[cfg(target_os = "linux")]
use expectrl::Expect;

use crate::shared::{SessionFixture, task_json};

#[test]
fn accepted_append_reaches_the_note_and_dispatched_prompt() {
    let fixture = SessionFixture::new();

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
            "--append",
            "one more thing in the moment",
        ])
        .env("PATH", &fixture.child_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
        .assert()
        .success();

    let task = task_json(&fixture.database, "PWF-0001");
    let prompt = task["prompt"].as_str().expect("task prompt");
    assert!(prompt.contains("one more thing in the moment"));

    let argv = fs::read_to_string(&fixture.tmux_log_path).unwrap();
    assert!(argv.contains("one more thing in the moment"));
}

#[test]
#[cfg(target_os = "linux")]
fn declined_append_leaves_the_note_unchanged_and_does_not_dispatch() {
    let fixture = SessionFixture::new();
    let task_before = task_json(&fixture.database, "PWF-0001");

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
            "--append",
            "declined context",
        ])
        .env("PATH", &fixture.child_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
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

    assert_eq!(task_json(&fixture.database, "PWF-0001"), task_before);
    let tmux_log = fs::read_to_string(&fixture.tmux_log_path).unwrap();
    assert!(!tmux_log.contains("new-window"));
}
