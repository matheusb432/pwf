use assert_cmd::prelude::OutputAssertExt as _;
#[cfg(target_os = "linux")]
use expectrl::Expect;

#[cfg(unix)]
use crate::support::{CommandTestExt, style::color_rgb};
use crate::support::{ManagedProject, project_id};

#[test]
#[cfg(unix)]
fn note_list_colors_follow_verification_and_custom_settings() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo").unwrap();
    fixture
        .database
        .command()
        .args(["note", "add", "foo", "sample / evidence"])
        .assert()
        .success();
    let configured = "[colors.note]\nactive = \"#010203\"\nverified = \"#040506\"\n";
    for (config, edit, color) in [
        ("", None, color_rgb(100, 149, 237)),
        ("", Some("--verified"), color_rgb(163, 230, 53)),
        ("", Some("--remove-verified"), color_rgb(100, 149, 237)),
        (configured, None, color_rgb(1, 2, 3)),
        (configured, Some("--verified"), color_rgb(4, 5, 6)),
        (configured, Some("--remove-verified"), color_rgb(1, 2, 3)),
    ] {
        fixture.database.write_user_config(config).unwrap();
        if let Some(flag) = edit {
            let mut command = fixture.database.command();
            command.args(["note", "edit", "foo", "1", flag]);
            if flag == "--verified" {
                command.arg("2026-09-08");
            }
            command.assert().success();
        }
        let output = fixture
            .database
            .command_args(&["note", "list", "foo"])
            .color()
            .success_stdout();
        assert_eq!(output, format!("{color}FOO-NOTE-0001{color:#} :: sample\n"));
        let output = fixture
            .database
            .command_args(&["note", "list", "foo"])
            .success_stdout();
        assert_eq!(output, "FOO-NOTE-0001 :: sample\n");
    }
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
