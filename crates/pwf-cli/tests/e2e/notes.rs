use std::process::Output;

use crate::support::{ManagedProject, project_id};

fn run(fixture: &ManagedProject, arguments: &[&str]) -> Output {
    fixture
        .database
        .command()
        .args(arguments)
        .output()
        .expect("run pwf process")
}

fn successful_stdout(fixture: &ManagedProject, arguments: &[&str]) -> String {
    let output = run(fixture, arguments);
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn note_lifecycle_is_observable_through_the_cli() {
    let fixture = ManagedProject::new(&project_id("PWF"), "pwf");

    let added = successful_stdout(
        &fixture,
        &[
            "note",
            "add",
            "pwf",
            "--title",
            "CLI contracts expose owned semantics",
            "--content",
            "Keep process behavior at the process boundary.",
        ],
    );
    assert!(added.contains("PWF-NOTE-0001"), "{added}");

    let listed = successful_stdout(&fixture, &["note", "pwf"]);
    assert!(listed.contains("CLI contracts expose owned semantics"));

    successful_stdout(
        &fixture,
        &[
            "note",
            "update",
            "PWF",
            "1",
            "CLI boundaries expose owned semantics",
        ],
    );
    let listed = successful_stdout(&fixture, &["note", "pwf"]);
    assert!(listed.contains("CLI boundaries expose owned semantics"));

    successful_stdout(&fixture, &["note", "remove", "pwf", "1"]);
    let listed = successful_stdout(&fixture, &["note", "pwf"]);
    assert!(!listed.contains("CLI boundaries expose owned semantics"));
}
