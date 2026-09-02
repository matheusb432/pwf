use assert_cmd::prelude::OutputAssertExt as _;
#[cfg(target_os = "linux")]
use expectrl::Expect;

use crate::support::{ManagedProject, assert_success, command, project_id, task_id, task_json};

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
fn list_priority_filters_and_renders_the_selected_tier() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    for (title, priority) in [("low task", "low"), ("highest task", "highest")] {
        fixture
            .database
            .command()
            .args([
                "add",
                "foo-bar",
                "--title",
                title,
                "--goal",
                "exercise priority listing",
                "--priority",
                priority,
            ])
            .assert()
            .success();
    }

    let listed = fixture
        .database
        .command()
        .args([
            "list",
            "--project",
            "foo-bar",
            "--all",
            "--long",
            "--priority",
            "highest",
        ])
        .output()
        .unwrap();

    assert!(listed.status.success());
    let output = String::from_utf8(listed.stdout).unwrap();
    assert!(output.contains("FOO-0002"), "{output}");
    assert!(!output.contains("FOO-0001"), "{output}");
    assert!(output.contains("priority: highest"), "{output}");
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
        "--priority",
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
        "--priority",
        "--remove-priority",
    ] {
        assert!(edit_help.contains(flag), "missing {flag}:\n{edit_help}");
    }

    let list = command().args(["task", "list", "--help"]).output().unwrap();
    assert!(list.status.success());
    let list_help = String::from_utf8(list.stdout).unwrap();
    assert!(
        list_help.contains("--priority"),
        "missing --priority:\n{list_help}"
    );

    let dag = command().args(["task", "dag", "--help"]).output().unwrap();
    assert!(dag.status.success());
    let dag_help = String::from_utf8(dag.stdout).unwrap();
    for contract in [
        "--depth <N>",
        "--mode <MODE>",
        "--status <STATUS>",
        "--with <WITH>",
        "[default: blocked-by]",
        "[default: all]",
    ] {
        assert!(
            dag_help.contains(contract),
            "missing {contract}:\n{dag_help}"
        );
    }
}

#[test]
fn task_dag_rejects_zero_depth_before_connecting() {
    let output = command()
        .args(["task", "dag", "FOO-0001", "--depth", "0"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("0 is not in 1.."), "{stderr}");
}

#[test]
fn task_dag_renders_compact_optional_status_colored_labels() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args([
            "task",
            "add",
            "foo-bar",
            "--title",
            "prepare graph data",
            "--goal",
            "supply the dependency",
        ])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args(["task", "done", "FOO-0001"])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args([
            "task",
            "add",
            "foo-bar",
            "--title",
            "render graph view",
            "--goal",
            "show the dependency",
            "--blocked-by",
            "FOO-0001",
        ])
        .assert()
        .success();

    let compact = fixture
        .database
        .command()
        .args(["task", "dag", "FOO-0002"])
        .output()
        .unwrap();

    assert_success(&compact, "render compact task DAG");
    assert!(compact.stderr.is_empty());
    let compact = String::from_utf8(compact.stdout).unwrap();
    assert!(compact.contains("FOO-0001"));
    assert!(compact.contains("FOO-0002"));
    assert!(!compact.contains("prepare graph data"), "{compact}");
    assert!(!compact.contains("render graph view"), "{compact}");
    assert!(!compact.contains("[active]"), "{compact}");
    assert!(!compact.contains("[done]"), "{compact}");
    assert!(compact.contains('▸'));
    assert!(compact.contains('┌'), "blocker must use a rectangular node");
    assert!(compact.contains('╭'), "root must use a rounded node");

    let with_title = fixture
        .database
        .command()
        .args(["task", "dag", "--with", "title", "FOO-0002"])
        .output()
        .unwrap();
    assert_success(&with_title, "render task DAG titles");
    let with_title = String::from_utf8(with_title.stdout).unwrap();
    assert!(with_title.contains("FOO-0001 prepare graph data"));
    assert!(with_title.contains("FOO-0002 render graph view"));
    assert!(!with_title.contains("[active]"), "{with_title}");
    assert!(!with_title.contains("[done]"), "{with_title}");

    let with_status = fixture
        .database
        .command()
        .args(["task", "dag", "FOO-0002", "--with", "status"])
        .output()
        .unwrap();
    assert_success(&with_status, "render task DAG statuses");
    let with_status = String::from_utf8(with_status.stdout).unwrap();
    assert!(with_status.contains("FOO-0001 [done]"));
    assert!(with_status.contains("FOO-0002 [active]"));
    assert!(!with_status.contains("prepare graph data"), "{with_status}");
    assert!(!with_status.contains("render graph view"), "{with_status}");

    let colored = fixture
        .database
        .command()
        .env_remove("NO_COLOR")
        .env("CLICOLOR_FORCE", "1")
        .args(["task", "dag", "FOO-0002"])
        .output()
        .unwrap();
    assert_success(&colored, "render colored task DAG");
    let colored = String::from_utf8(colored.stdout).unwrap();
    assert!(
        colored.contains("\u{1b}[32mFOO-0001\u{1b}[0m"),
        "{colored:?}"
    );
    assert!(
        colored.contains("\u{1b}[34mFOO-0002\u{1b}[0m"),
        "{colored:?}"
    );
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
