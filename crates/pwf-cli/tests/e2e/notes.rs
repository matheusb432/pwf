use std::process::Output;

use crate::support::{ManagedProject, project_id};

fn run(fixture: &ManagedProject, arguments: &[&str]) -> std::io::Result<Output> {
    fixture.database.command().args(arguments).output()
}

fn successful_stdout(fixture: &ManagedProject, arguments: &[&str]) -> anyhow::Result<String> {
    let output = run(fixture, arguments)?;
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    Ok(String::from_utf8(output.stdout)?)
}

#[test]
fn note_lifecycle_is_observable_through_the_cli() -> anyhow::Result<()> {
    let fixture = ManagedProject::new(&project_id("FOO")?, "foo")?;

    let added = successful_stdout(
        &fixture,
        &[
            "note",
            "add",
            "foo",
            "--title",
            "CLI contracts expose owned semantics",
            "--content",
            "Keep process behavior at the process boundary.",
        ],
    )?;
    assert!(added.contains("FOO-NOTE-0001"), "{added}");

    let listed = successful_stdout(&fixture, &["note", "foo"])?;
    assert!(listed.contains("CLI contracts expose owned semantics"));

    successful_stdout(
        &fixture,
        &[
            "note",
            "update",
            "FOO",
            "1",
            "CLI boundaries expose owned semantics",
        ],
    )?;
    let listed = successful_stdout(&fixture, &["note", "foo"])?;
    assert!(listed.contains("CLI boundaries expose owned semantics"));

    successful_stdout(&fixture, &["note", "remove", "foo", "1"])?;
    let listed = successful_stdout(&fixture, &["note", "foo"])?;
    assert!(!listed.contains("CLI boundaries expose owned semantics"));
    Ok(())
}
