use assert_cmd::prelude::OutputAssertExt as _;

use crate::support::{ManagedProject, command, project_id};

#[test]
fn note_add_help_exposes_only_title_and_content_inputs() {
    let output = command().args(["note", "add", "--help"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("--title <TITLE>"), "{stdout}");
    assert!(stdout.contains("--content <CONTENT>"), "{stdout}");
}

#[test]
fn note_add_rejects_incomplete_or_mixed_input_modes() {
    let fixture = ManagedProject::new(&project_id("PWF"), "pwf");

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
