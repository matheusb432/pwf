use assert_cmd::prelude::OutputAssertExt as _;
#[cfg(target_os = "linux")]
use expectrl::Expect;

use crate::support::{ManagedProject, command, project_id};

#[test]
fn note_help_exposes_explicit_add_and_edit_fields_and_retires_update() {
    let output = command().args(["note", "add", "--help"]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for flag in [
        "--title <TITLE>",
        "--content <CONTENT>",
        "--domain <DOMAIN>",
        "--tag <TAG>",
        "--source <SOURCE>",
        "--verified <VERIFIED>",
        "--date <DATE>",
    ] {
        assert!(stdout.contains(flag), "missing {flag}:\n{stdout}");
    }
    assert!(!stdout.contains("--why"), "unexpected --why:\n{stdout}");

    let output = command().args(["note", "edit", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for flag in [
        "--title <TITLE>",
        "--content <CONTENT>",
        "--domain <DOMAIN>",
        "--remove-domain",
        "--add-tag <TAG>",
        "--remove-tags",
        "--add-source <SOURCE>",
        "--remove-sources",
        "--verified <VERIFIED>",
        "--remove-verified",
    ] {
        assert!(stdout.contains(flag), "missing {flag}:\n{stdout}");
    }
    assert!(!stdout.contains("--why"), "unexpected --why:\n{stdout}");
    assert!(
        !stdout.contains("--remove-why"),
        "unexpected --remove-why:\n{stdout}"
    );
    assert!(!stdout.contains("--date"), "unexpected --date:\n{stdout}");

    command()
        .args(["note", "update", "--help"])
        .assert()
        .failure();
}

#[test]
fn note_add_rejects_incomplete_or_mixed_input_modes() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo").unwrap();

    let output = fixture
        .database
        .command()
        .args(["note", "add", "foo", "missing separator"])
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
        .args(["note", "add", "foo", "--title", "missing content"])
        .assert()
        .failure();
    fixture
        .database
        .command()
        .args([
            "note",
            "add",
            "foo",
            "title / content",
            "--title",
            "duplicate title",
            "--content",
            "duplicate content",
        ])
        .assert()
        .failure();
}

#[test]
fn note_remove_requires_yes_without_a_terminal_and_preserves_the_note() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo").unwrap();
    fixture
        .database
        .command()
        .args(["note", "add", "foo", "remember milk / buy it today"])
        .assert()
        .success();

    let output = fixture
        .database
        .command()
        .args(["note", "remove", "foo", "1"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("interactive confirmation requires a terminal"),
        "{stderr}"
    );
    let listed = fixture
        .database
        .command()
        .args(["note", "list", "foo"])
        .output()
        .unwrap();
    assert!(listed.status.success());
    assert!(
        String::from_utf8(listed.stdout)
            .unwrap()
            .contains("remember milk")
    );
}

#[test]
#[cfg(target_os = "linux")]
fn note_remove_prompt_renders_note_context_before_deletion() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo").unwrap();
    fixture
        .database
        .command()
        .args(["note", "add", "foo", "remember milk / buy it today"])
        .assert()
        .success();

    let mut command = fixture.database.command();
    command
        .args(["note", "remove", "foo", "1"])
        .env("NO_COLOR", "1");

    let mut session = expectrl::Session::spawn(command).unwrap();
    session.set_expect_timeout(Some(std::time::Duration::from_secs(10)));
    session.expect("Confirm note removal").unwrap();
    session.expect("Note").unwrap();
    session.expect("FOO-NOTE-0001").unwrap();
    session.expect("Title").unwrap();
    session.expect("remember milk").unwrap();
    session.expect("Project").unwrap();
    session.expect("foo").unwrap();
    session.expect("(y/n)").unwrap();
    session.expect("no").unwrap();
    session.send("y").unwrap();
    session.expect(expectrl::Eof).unwrap();
    assert!(matches!(
        session.get_process().wait().unwrap(),
        expectrl::process::unix::WaitStatus::Exited(_, 0)
    ));

    let listed = fixture
        .database
        .command()
        .args(["note", "list", "foo"])
        .output()
        .unwrap();
    assert!(listed.status.success());
    assert!(
        !String::from_utf8(listed.stdout)
            .unwrap()
            .contains("remember milk")
    );
}
