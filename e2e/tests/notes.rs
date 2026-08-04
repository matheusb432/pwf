use assert_cmd::prelude::OutputAssertExt as _;

use crate::shared::{ManagedProject, project_id, task_id, task_json};

#[test]
fn note_lifecycle_does_not_change_tasks() {
    let fixture = ManagedProject::new(project_id("PWF"), "pwf");
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
    let task_before = task_json(&fixture.database, &task_id("PWF-0001"));

    fixture
        .database
        .command()
        .args([
            "note",
            "add",
            "pwf",
            "--title",
            "CLI contracts should expose owned semantics",
            "--content",
            " \nA CLI flag needs a binary test only for an owned contract.\n\n- Preserve the process boundary.\n ",
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
        "A CLI flag needs a binary test only for an owned contract.\n\n- Preserve the process boundary.",
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
    assert_eq!(
        task_json(&fixture.database, &task_id("PWF-0001")),
        task_before
    );
}

#[test]
fn note_add_positional_shorthand_splits_once_and_preserves_slashes_in_content() {
    let fixture = ManagedProject::new(project_id("PWF"), "pwf");

    fixture
        .database
        .command()
        .args([
            "note",
            "add",
            "pwf",
            "using tokio::join!() / Run independent futures concurrently. Paths like docs/async.md stay intact.",
            "--date",
            "2026-08-03",
        ])
        .assert()
        .success()
        .stdout("Added PWF-NOTE-0001 :: using tokio::join!()\n\n");

    assert_eq!(
        fixture.note_markdown("PWF-NOTE-0001"),
        concat!(
            "---\n",
            "type: note\n",
            "project: pwf\n",
            "created: 2026-08-03\n",
            "---\n\n",
            "# using tokio::join!()\n\n",
            "Run independent futures concurrently. Paths like docs/async.md stay intact.\n",
        )
    );
}

#[test]
fn note_add_help_exposes_only_title_and_content_inputs() {
    let fixture = ManagedProject::new(project_id("PWF"), "pwf");

    let output = fixture
        .database
        .command()
        .args(["note", "add", "--help"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("--title <TITLE>"), "{stdout}");
    assert!(stdout.contains("--content <CONTENT>"), "{stdout}");
    assert!(!stdout.contains("--topic"), "{stdout}");
    assert!(!stdout.contains("--tldr"), "{stdout}");
}

#[test]
fn note_add_rejects_incomplete_or_mixed_input_modes() {
    let fixture = ManagedProject::new(project_id("PWF"), "pwf");

    let output = fixture
        .database
        .command()
        .args(["note", "add", "pwf", "missing separator"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("Positional note must contain ' / ' between its title and content."),
        "{stderr}"
    );
    fixture
        .database
        .command()
        .args(["note", "add", "pwf", "--title", "missing content"])
        .assert()
        .failure();
    fixture
        .database
        .command()
        .args([
            "note",
            "add",
            "pwf",
            "title / content",
            "--title",
            "duplicate title",
            "--content",
            "duplicate content",
        ])
        .assert()
        .failure();
}
