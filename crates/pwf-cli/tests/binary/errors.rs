#[cfg(target_os = "linux")]
use expectrl::Expect as _;

use super::support::{ManagedProject, command, project_id};

#[test]
fn unknown_root_commands_use_clap_diagnostics_without_connecting() {
    let runtime = tempfile::tempdir().unwrap();
    let output = command()
        .env("PWF_RUNTIME_DIR", runtime.path())
        .arg("sample-project")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        error.starts_with("error: unrecognized subcommand 'sample-project'\n"),
        "{error}"
    );
    assert!(error.contains("Usage: pwf <COMMAND>"), "{error}");
    assert!(
        error.contains("For more information, try '--help'."),
        "{error}"
    );
}

#[test]
fn runtime_errors_use_clap_with_the_selected_command_context() {
    let runtime = tempfile::tempdir().unwrap();
    let output = command()
        .env_remove("NO_COLOR")
        .env("PWF_RUNTIME_DIR", runtime.path())
        .args(["task", "list"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        error.starts_with("error: Cannot connect to pwf-server."),
        "{error}"
    );
    assert!(error.contains("Usage: pwf task list"), "{error}");
    assert!(
        error.contains("For more information, try '--help'."),
        "{error}"
    );
    assert!(!error.contains('\x1b'));
}

#[test]
#[cfg(target_os = "linux")]
fn clap_errors_color_only_terminal_stderr_and_respect_no_color() {
    let fixture = ManagedProject::new(&project_id("ABC").unwrap(), "sample-project").unwrap();
    for args in [["task", "unknown"], ["get", "ABC-0001"]] {
        for no_color in [false, true] {
            let mut command = fixture.database.command_args(&args);
            command.env("TERM", "xterm-256color");
            if !no_color {
                command.env_remove("NO_COLOR");
            }
            let mut session = expectrl::Session::spawn(command).unwrap();
            session.set_expect_timeout(Some(std::time::Duration::from_secs(10)));
            let capture = session.expect(expectrl::Eof).unwrap();
            let output = String::from_utf8_lossy(capture.get(0).unwrap());
            let plain = dialoguer::console::strip_ansi_codes(&output);
            assert!(plain.starts_with("error: "), "{output:?}");
            assert!(plain.contains("Usage: pwf task"), "{output:?}");
            assert_eq!(output.contains('\x1b'), !no_color, "{output:?}");
            if !no_color {
                let style = clap_cargo::style::CLAP_STYLING.get_error();
                assert!(
                    output.starts_with(&format!("{style}error:{style:#}")),
                    "{output:?}"
                );
            }
            assert!(matches!(
                session.get_process().wait().unwrap(),
                expectrl::process::unix::WaitStatus::Exited(_, 1 | 2)
            ));
        }
    }
}

#[test]
fn unknown_project_ids_suggest_only_a_unique_eligible_id() {
    use super::support::CommandTestExt as _;
    let fixture = ManagedProject::new(&project_id("ABC").unwrap(), "sample-project").unwrap();
    for args in [
        vec!["bac"],
        vec!["task", "list", "--project", "bac"],
        vec!["task", "add", "bac", "sample"],
        vec!["clone", "ABC-0001", "--project", "bac"],
        vec!["note", "add", "bac", "sample / content"],
        vec!["note", "bac"],
    ] {
        let output = fixture.database.command_args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.starts_with("error: invalid value 'bac'"), "{error}");
        assert!(
            error.contains("tip: a similar value exists: 'abc'"),
            "{error}"
        );
        assert!(!error.contains("sample-project"), "{error}");
        assert!(!error.contains("possible values"), "{error}");
        assert!(!error.contains('\x1b'));
    }
    assert!(
        fixture
            .database
            .command_args(&["abc", "--long=json"])
            .success_json()
            .as_array()
            .unwrap()
            .is_empty()
    );
    let other = tempfile::tempdir().unwrap();
    let tasks = other.path().join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    fixture.database.add_directory_project(
        &project_id("ABD").unwrap(),
        "second-project",
        other.path(),
        &tasks,
    );
    let error = fixture.database.command_args(&["abe"]).output().unwrap();
    let error = String::from_utf8(error.stderr).unwrap();
    assert!(!error.contains("tip:"), "{error}");
    fixture
        .database
        .command_args(&["project", "pause", "ABD", "--json"])
        .success_json();
    let error = fixture.database.command_args(&["abe"]).output().unwrap();
    let error = String::from_utf8(error.stderr).unwrap();
    assert!(
        error.contains("tip: a similar value exists: 'abc'"),
        "{error}"
    );
    for input in ["zzz", "abd"] {
        let output = fixture.database.command_args(&[input]).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        let error = String::from_utf8(output.stderr).unwrap();
        if input == "zzz" {
            assert!(!error.contains("tip:"), "{error}");
        }
        assert!(!error.contains("second-project"), "{error}");
    }
}

#[test]
fn explicit_project_arguments_reject_titles_before_connecting() {
    let runtime = tempfile::tempdir().unwrap();
    for args in [
        vec!["task", "list", "--project", "sample-project"],
        vec!["add", "sample-project", "sample"],
        vec!["clone", "ABC-0001", "--project", "sample-project"],
        vec!["note", "list", "sample-project"],
    ] {
        let output = command()
            .env("PWF_RUNTIME_DIR", runtime.path())
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            error.starts_with("error: invalid value 'sample-project'"),
            "{error}"
        );
        assert!(error.contains("two to four ASCII letters"), "{error}");
    }
}
