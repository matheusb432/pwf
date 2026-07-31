use assert_cmd::prelude::OutputAssertExt as _;

use crate::shared::{ManagedProject, task_json};

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
        .args([
            "note",
            "add",
            "pwf",
            "--topic",
            "CLI contracts should expose owned semantics",
            "--tldr",
            "A CLI flag needs a binary test only for an owned contract.",
            "--why",
            "This protects real process-boundary failures.",
            "--domain",
            "testing",
            "--tag",
            "cli",
            "--tag",
            "testing",
            "--source",
            "PWF-0165 implementation evidence",
            "--verified",
            "2026-07-30",
            "--date",
            "2026-07-30",
        ])
        .assert()
        .success();
    let note_before_update = fixture.note_markdown("PWF-NOTE-0001");
    for expected in [
        "created: 2026-07-30",
        "domain: \"testing\"",
        "tags: [\"cli\", \"testing\"]",
        "sources: [\"PWF-0165 implementation evidence\"]",
        "verified: \"2026-07-30\"",
        "# CLI contracts should expose owned semantics",
        "> **TL;DR:** A CLI flag needs a binary test only for an owned contract.",
        "## Why it matters",
        "This protects real process-boundary failures.",
        "## Sources",
        "- PWF-0165 implementation evidence",
    ] {
        assert!(
            note_before_update.contains(expected),
            "note did not contain {expected:?}:\n{note_before_update}"
        );
    }
    fixture
        .database
        .command()
        .args([
            "note",
            "update",
            "PWF",
            "1",
            "CLI boundaries expose owned semantics",
        ])
        .assert()
        .success();
    assert_eq!(
        fixture.note_markdown("PWF-NOTE-0001"),
        note_before_update.replacen(
            "# CLI contracts should expose owned semantics",
            "# CLI boundaries expose owned semantics",
            1,
        )
    );

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
            .contains("CLI boundaries expose owned semantics")
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
            .contains("CLI boundaries expose owned semantics")
    );
    assert_eq!(task_json(&fixture.database, "PWF-0001"), task_before);
}
