#![cfg(unix)]

use std::fs;

use assert_cmd::prelude::OutputAssertExt as _;

use crate::support::{SessionFixture, task_id, task_json};

#[test]
fn session_dispatches_the_task_without_persisting_ephemeral_context() {
    let fixture = SessionFixture::new().unwrap();
    let task_before = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();

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
            "--push-prompt",
            "one more thing in the moment",
        ])
        .env("PATH", &fixture.child_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
        .assert()
        .success();

    let dispatch = fs::read_to_string(&fixture.tmux_log_path).unwrap();
    assert!(dispatch.contains("new-window"), "{dispatch:?}");
    assert!(
        dispatch.contains("one more thing in the moment"),
        "{dispatch:?}"
    );
    assert_eq!(
        task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap(),
        task_before
    );
}
