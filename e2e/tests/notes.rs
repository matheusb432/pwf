use assert_cmd::prelude::OutputAssertExt as _;

use crate::common::{ManagedProject, task_json};

#[test]
fn note_lifecycle_does_not_change_pending_work() {
    let fixture = ManagedProject::new("PWF", "pwf");
    fixture
        .database
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
    let task_before = task_json(&fixture.database, "PWF-0001");

    fixture
        .database
        .command()
        .args(["note", "add", "pwf", "remember the milk"])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args(["note", "update", "PWF", "1", "remember oat milk"])
        .assert()
        .success();

    let listed = fixture
        .database
        .command()
        .args(["note", "pwf"])
        .output()
        .unwrap();
    assert!(listed.status.success());
    assert!(
        String::from_utf8(listed.stdout)
            .unwrap()
            .contains("remember oat milk")
    );

    fixture
        .database
        .command()
        .args(["note", "remove", "pwf", "1"])
        .assert()
        .success();

    let listed = fixture
        .database
        .command()
        .args(["note", "pwf"])
        .output()
        .unwrap();
    assert!(listed.status.success());
    assert!(
        !String::from_utf8(listed.stdout)
            .unwrap()
            .contains("remember oat milk")
    );
    assert_eq!(task_json(&fixture.database, "PWF-0001"), task_before);
}
