#![cfg(unix)]

use std::fs;

use assert_cmd::prelude::OutputAssertExt as _;

use crate::support::{SessionFixture, task_id, task_json};

#[test]
fn session_dispatches_multiple_tasks_without_persisting_ephemeral_context() {
    let fixture = SessionFixture::new().unwrap();
    fixture.add_task("do the other thing");
    let first_id = task_id("FOO-0001").unwrap();
    let second_id = task_id("FOO-0002").unwrap();
    let first_before = task_json(&fixture.database, &first_id).unwrap();
    let second_before = task_json(&fixture.database, &second_id).unwrap();

    fixture
        .database
        .command()
        .args([
            "session",
            "--id",
            "foo2,foo1",
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
    assert!(dispatch.contains("-n foo1,foo2"), "{dispatch:?}");
    assert!(
        dispatch.contains("one more thing in the moment"),
        "{dispatch:?}"
    );
    assert_eq!(dispatch.matches("<pwf_session_context>").count(), 1);
    assert_eq!(dispatch.matches("<pwf_task>").count(), 2);
    assert!(
        dispatch.find("do the other thing").unwrap() < dispatch.find("do the thing").unwrap(),
        "{dispatch:?}"
    );
    assert_eq!(
        task_json(&fixture.database, &first_id).unwrap(),
        first_before
    );
    assert_eq!(
        task_json(&fixture.database, &second_id).unwrap(),
        second_before
    );
}
