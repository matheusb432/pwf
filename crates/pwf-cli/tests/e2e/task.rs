use assert_cmd::prelude::OutputAssertExt as _;
use serde_json::{Value, json};

use crate::support::{ManagedProject, project_id, task_id, task_json};

#[test]
fn completing_a_task_can_create_a_review_task() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args([
            "add",
            "foo-bar",
            "--title",
            "implementation",
            "--goal",
            "implementation work",
        ])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args(["done", "FOO-0001", "--commits", "a..b", "--review"])
        .assert()
        .success();

    let review = task_json(&fixture.database, &task_id("FOO-0002").unwrap()).unwrap();
    assert_eq!(review["status"], "active");
    assert_eq!(review["section"], "Human");
    assert_eq!(review["title"], "review foo-0001, commits; a..b");
    assert_eq!(review["prompt"], "## Goals");

    let listed = fixture
        .database
        .command()
        .args(["list", "--status", "active", "--all"])
        .output()
        .unwrap();
    assert!(listed.status.success());
    let listed = String::from_utf8(listed.stdout).unwrap();
    assert!(listed.contains("FOO-0002"), "{listed}");
}

#[test]
fn task_lifecycle_is_observable_through_json() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args([
            "add",
            "foo-bar",
            "--title",
            "blocker",
            "--goal",
            "blocking work",
        ])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args([
            "add",
            "foo-bar",
            "--title",
            "just done",
            "--goal",
            "complete the work",
            "--blocked-by",
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

    let active = task_json(&fixture.database, &task_id("FOO-0002").unwrap()).unwrap();
    assert_eq!(active["id"], "FOO-0002");
    assert_eq!(active["project"], "foo-bar");
    assert_eq!(active["title"], "just done");
    assert_eq!(active["status"], "active");
    assert_eq!(active["tags"], json!(["cli", "sqlite"]));
    assert_eq!(active["effort"], "high");
    assert_eq!(active["blocked_by"], json!(["FOO-0001"]));

    fixture
        .database
        .command()
        .args([
            "edit",
            "FOO-0002",
            "--prompt",
            "ship it / preserve the revised goal",
            "--remove-tags",
            "--add-tag",
            "rust",
            "--remove-blocked-by",
            "--remove-effort",
        ])
        .assert()
        .success();
    let updated = task_json(&fixture.database, &task_id("FOO-0002").unwrap()).unwrap();
    assert_eq!(updated["title"], "ship it");
    assert_eq!(updated["tags"], json!(["rust"]));
    assert_eq!(updated["effort"], Value::Null);
    assert_eq!(updated["blocked_by"], Value::Null);

    fixture
        .database
        .command()
        .args(["done", "FOO-0002", "--commits", "a..b"])
        .assert()
        .success();
    let done = task_json(&fixture.database, &task_id("FOO-0002").unwrap()).unwrap();
    assert_eq!(done["status"], "done");
    assert!(done["completed"].as_str().is_some());
    assert_eq!(done["commits"], "a..b");

    fixture
        .database
        .command()
        .args(["reopen", "FOO-0002", "--yes"])
        .assert()
        .success();
    let reopened = task_json(&fixture.database, &task_id("FOO-0002").unwrap()).unwrap();
    assert_eq!(reopened["status"], "active");
    assert_eq!(reopened["completed"], Value::Null);
    assert_eq!(reopened["commits"], Value::Null);
}
