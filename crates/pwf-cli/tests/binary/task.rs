use assert_cmd::prelude::OutputAssertExt as _;
#[cfg(target_os = "linux")]
use expectrl::Expect;

use crate::support::{ManagedProject, command, project_id, task_id, task_json};

#[test]
fn machine_add_maps_each_explicit_value_without_parsing_lane_markers() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();

    fixture
        .database
        .command()
        .args([
            "task",
            "add",
            "foo-bar",
            "--title",
            "ship parser",
            "--goal",
            "preserve the authored goal",
            "--goal",
            "keep /d as literal text",
            "--context",
            "the machine supplies independent values",
            "--constraint",
            "preserve shorthand mode",
            "--done-when",
            "both input modes are covered",
        ])
        .assert()
        .success();

    let task = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();
    assert_eq!(task["title"], "ship parser");
    assert_eq!(
        task["prompt"],
        "## Goals\n\n- preserve the authored goal\n- keep /d as literal text\n\n## Context\n\n- the machine supplies independent values\n\n## Constraints\n\n- preserve shorthand mode\n\n## Done When\n\n- both input modes are covered"
    );
}

#[test]
fn root_edit_replaces_lane_collections_with_explicit_remove_then_add_actions() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args([
            "task",
            "add",
            "foo-bar",
            "--title",
            "original task",
            "--goal",
            "old goal",
            "--context",
            "old context",
            "--constraint",
            "old constraint",
            "--done-when",
            "old outcome",
        ])
        .assert()
        .success();

    fixture
        .database
        .command()
        .args([
            "edit",
            "FOO-0001",
            "--title",
            "edited task",
            "--remove-goals",
            "--add-goal",
            "new goal /c stays literal",
            "--remove-contexts",
            "--add-context",
            "new context",
            "--remove-constraints",
            "--add-constraint",
            "new constraint",
            "--remove-done-whens",
            "--add-done-when",
            "new outcome",
        ])
        .assert()
        .success();

    let task = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();
    assert_eq!(task["title"], "edited task");
    assert_eq!(
        task["prompt"],
        "## Goals\n\n- new goal /c stays literal\n\n## Context\n\n- new context\n\n## Constraints\n\n- new constraint\n\n## Done When\n\n- new outcome"
    );
}

#[test]
fn task_help_exposes_only_the_supported_add_and_edit_contract() {
    let add = command().args(["task", "add", "--help"]).output().unwrap();
    assert!(add.status.success());
    let add_help = String::from_utf8(add.stdout).unwrap();
    for flag in [
        "--title",
        "--goal",
        "--context",
        "--constraint",
        "--done-when",
        "--blocked-by",
    ] {
        assert!(add_help.contains(flag), "missing {flag}:\n{add_help}");
    }

    let edit = command().args(["task", "edit", "--help"]).output().unwrap();
    assert!(edit.status.success());
    let edit_help = String::from_utf8(edit.stdout).unwrap();
    for flag in [
        "--add-goal",
        "--remove-goals",
        "--add-context",
        "--remove-contexts",
        "--add-constraint",
        "--remove-constraints",
        "--add-done-when",
        "--remove-done-whens",
        "--add-blocked-by",
        "--remove-blocked-by",
        "--add-tag",
        "--remove-tags",
        "--remove-effort",
    ] {
        assert!(edit_help.contains(flag), "missing {flag}:\n{edit_help}");
    }
}

#[test]
fn shorthand_and_machine_lane_inputs_conflict_before_mutation() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args(["add", "foo-bar", "original / keep this goal"])
        .assert()
        .success();
    let before = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();

    fixture
        .database
        .command()
        .args([
            "edit",
            "FOO-0001",
            "--prompt",
            "replacement / replacement goal",
            "--add-goal",
            "ambiguous goal",
        ])
        .assert()
        .failure();

    assert_eq!(
        task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap(),
        before
    );
}

#[test]
#[cfg(target_os = "linux")]
fn remove_prompt_identifies_closed_status_before_deletion() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args([
            "add",
            "foo-bar",
            "--title",
            "completed work",
            "--goal",
            "remove completed work",
        ])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args(["done", "FOO-0001"])
        .assert()
        .success();

    let mut command = fixture.database.command();
    command.args(["remove", "FOO-0001"]).env("NO_COLOR", "1");

    let mut session = expectrl::Session::spawn(command).unwrap();
    session.set_expect_timeout(Some(std::time::Duration::from_secs(10)));
    session.expect("Task").unwrap();
    session.expect("FOO-0001").unwrap();
    session.expect("Title").unwrap();
    session.expect("completed work").unwrap();
    session.expect("Status").unwrap();
    session.expect("done").unwrap();
    session.expect("(y/n)").unwrap();
    session.expect("no").unwrap();
    session.send("y").unwrap();
    session.expect(expectrl::Eof).unwrap();
    assert!(matches!(
        session.get_process().wait().unwrap(),
        expectrl::process::unix::WaitStatus::Exited(_, 0)
    ));

    fixture
        .database
        .command()
        .args(["get", "FOO-0001", "--json"])
        .assert()
        .failure();
}
