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
fn colored_all_status_list_uses_color_instead_of_a_status_tag() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args([
            "add",
            "foo-bar",
            "--title",
            "orange task",
            "--goal",
            "render the configured color",
        ])
        .assert()
        .success();
    fixture
        .database
        .write_user_config("[colors]\nactive = \"#ff8700\"\n")
        .unwrap();

    fixture
        .database
        .command()
        .env("NO_COLOR", "1")
        .args(["list", "--project", "foo-bar", "--all"])
        .assert()
        .success()
        .stdout("FOO-0001 [active] :: orange task\n");

    for arguments in [
        ["foo-bar"].as_slice(),
        ["list", "--project", "foo-bar", "--all"].as_slice(),
    ] {
        let output = fixture
            .database
            .command()
            .env_remove("NO_COLOR")
            .env("CLICOLOR_FORCE", "1")
            .args(arguments)
            .output()
            .unwrap();

        assert!(output.status.success(), "{arguments:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert_eq!(
            stdout, "\u{1b}[1m\u{1b}[38;2;255;135;0mFOO-0001\u{1b}[0m :: orange task\n",
            "{arguments:?}"
        );
    }
}

#[test]
fn task_command_reports_invalid_user_config_with_its_path_and_cause() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .write_user_config("[colors]\nactive = \"#fff\"\n")
        .unwrap();

    let output = fixture.database.command().arg("foo-bar").output().unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(".config/pwf/config.toml"), "{stderr}");
    assert!(stderr.contains("`colors.active` is invalid"), "{stderr}");
    assert!(stderr.contains("#RRGGBB"), "{stderr}");
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
    assert!(!add_help.contains("--human"), "{add_help}");

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
    assert!(list_help.contains("--section <HEADER>"), "{list_help}");

    for command_name in ["done", "cancel"] {
        let output = command()
            .args(["task", command_name, "--help"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(!help.contains("--review"), "{help}");
    }

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
fn removed_section_workflow_flags_are_rejected_by_the_parser() {
    for arguments in [
        vec!["task", "add", "foo", "ship it", "--human"],
        vec!["task", "done", "FOO-0001", "--review"],
        vec![
            "task", "cancel", "FOO-0001", "--report", "obsolete", "--review",
        ],
    ] {
        let output = command().args(&arguments).output().unwrap();

        assert!(!output.status.success(), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("unexpected argument"),
            "{arguments:?}"
        );
    }
}

#[test]
fn task_list_rejects_section_with_all_before_connecting() {
    let output = command()
        .args(["task", "list", "--section", "Waiting on API", "--all"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("cannot be used with '--all'"), "{stderr}");
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
fn task_dag_renders_exact_compact_title_status_and_colored_output() {
    let fixture = task_dag_render_fixture();

    fixture
        .database
        .command()
        .env("NO_COLOR", "1")
        .args(["task", "dag", "FOO-0003"])
        .assert()
        .success()
        .stdout(include_bytes!("../fixtures/task_dag/compact.stdout").as_slice())
        .stderr(include_bytes!("../fixtures/task_dag/empty.stderr").as_slice());

    fixture
        .database
        .command()
        .env("NO_COLOR", "1")
        .args(["task", "dag", "--with", "title", "FOO-0003"])
        .assert()
        .success()
        .stdout(include_bytes!("../fixtures/task_dag/with_title.stdout").as_slice())
        .stderr(include_bytes!("../fixtures/task_dag/empty.stderr").as_slice());

    fixture
        .database
        .command()
        .env("NO_COLOR", "1")
        .args(["task", "dag", "FOO-0003", "--with", "status"])
        .assert()
        .success()
        .stdout(include_bytes!("../fixtures/task_dag/with_status.stdout").as_slice())
        .stderr(include_bytes!("../fixtures/task_dag/empty.stderr").as_slice());

    fixture
        .database
        .command()
        .env_remove("NO_COLOR")
        .env("CLICOLOR_FORCE", "1")
        .args(["task", "dag", "FOO-0003"])
        .assert()
        .success()
        .stdout(include_bytes!("../fixtures/task_dag/colored.stdout").as_slice())
        .stderr(include_bytes!("../fixtures/task_dag/empty.stderr").as_slice());
}

fn task_dag_render_fixture() -> ManagedProject {
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
            "cancel obsolete renderer",
            "--goal",
            "preserve cancelled task output",
        ])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args([
            "task",
            "cancel",
            "FOO-0002",
            "--report",
            "renderer no longer applies",
        ])
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
            "--blocked-by",
            "FOO-0002",
        ])
        .assert()
        .success();
    fixture
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
