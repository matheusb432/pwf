use assert_cmd::prelude::OutputAssertExt as _;
#[cfg(target_os = "linux")]
use expectrl::Expect;

use crate::support::{
    CommandTestExt, ManagedProject, command, project_id,
    style::{assert_plain, color_rgb},
    task_id, task_json,
};

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
#[cfg(unix)]
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
        .write_user_config("[colors.task]\nactive = \"#ff8700\"\n")
        .unwrap();

    let plain = fixture
        .database
        .command()
        .args(["list", "--project", "foo-bar", "--all"])
        .output()
        .unwrap();
    assert!(plain.status.success());
    let stdout = String::from_utf8(plain.stdout).unwrap();
    assert!(stdout.contains("FOO-0001"), "{stdout:?}");
    assert!(stdout.contains("[active]"), "{stdout:?}");
    assert!(stdout.contains("orange task"), "{stdout:?}");
    assert_plain(&stdout);

    for arguments in [
        ["foo-bar"].as_slice(),
        ["list", "--project", "foo-bar", "--all"].as_slice(),
    ] {
        let output = fixture
            .database
            .command()
            .color()
            .args(arguments)
            .output()
            .unwrap();

        assert!(output.status.success(), "{arguments:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains(&format!(
                "{orange}FOO-0001{orange:#}",
                orange = color_rgb(255, 135, 0)
            )),
            "{arguments:?}: {stdout:?}"
        );
        assert!(stdout.contains("orange task"), "{stdout:?}");
        assert!(!stdout.contains("[active]"), "{stdout:?}");
    }
}

#[test]
#[cfg(unix)]
fn task_command_reports_invalid_user_config_with_its_path_and_cause() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    let config_path = fixture
        .database
        .write_user_config("[colors.task]\nactive = \"#fff\"\n")
        .unwrap();

    let output = fixture.database.command().arg("foo-bar").output().unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(&*config_path.to_string_lossy()), "{stderr}");
    assert!(
        stderr.contains("`colors.task.active` is invalid"),
        "{stderr}"
    );
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
        .color()
        .args(["task", "dag", "FOO-0003"])
        .assert()
        .success()
        .stdout(format!(
            include_str!("../fixtures/task_dag/colored.stdout"),
            blue = color_rgb(100, 149, 237),
            green = color_rgb(163, 230, 53),
            red = color_rgb(255, 107, 138)
        ))
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
    session.expect("Deletion").unwrap();
    session.expect("hard delete").unwrap();
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

#[test]
#[cfg(unix)]
fn configured_list_order_and_priority_apply_to_every_list_spelling() -> anyhow::Result<()> {
    let fixture = ManagedProject::new(&project_id("FOO")?, "foo-bar")?;
    for (title, priority, effort) in [
        ("zeta", Some("low"), Some("high")),
        ("alpha", Some("highest"), Some("low")),
        ("beta", None, None),
    ] {
        let mut arguments = vec![
            "add",
            "foo-bar",
            "--title",
            title,
            "--goal",
            "exercise list ordering",
        ];
        if let Some(priority) = priority {
            arguments.extend(["--priority", priority]);
        }
        if let Some(effort) = effort {
            arguments.extend(["--effort", effort]);
        }
        fixture
            .database
            .command()
            .args(arguments)
            .assert()
            .success();
    }
    fixture
        .database
        .write_user_config("default_priority = \"highest\"\ndefault_sort_order = \"priority\"\n")?;
    for prefix in [
        vec!["task", "list", "--project", "foo-bar"],
        vec!["list", "--project", "foo-bar"],
        vec!["foo-bar"],
    ] {
        let output = fixture.database.command().args(&prefix).output()?;
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert_list_ids(
            &String::from_utf8(output.stdout)?,
            &["FOO-0003", "FOO-0002", "FOO-0001"],
        );
        let output = fixture
            .database
            .command()
            .args(prefix)
            .args(["--order", "title"])
            .output()?;
        assert!(output.status.success());
        assert_list_ids(
            &String::from_utf8(output.stdout)?,
            &["FOO-0002", "FOO-0003", "FOO-0001"],
        );
    }
    for (order, expected) in [
        ("priority:asc", ["FOO-0001", "FOO-0003", "FOO-0002"]),
        ("effort", ["FOO-0002", "FOO-0001", "FOO-0003"]),
        ("effort:desc", ["FOO-0001", "FOO-0002", "FOO-0003"]),
        ("title:desc", ["FOO-0001", "FOO-0003", "FOO-0002"]),
    ] {
        let output = fixture
            .database
            .command()
            .args(["list", "--project", "foo-bar", "--order", order])
            .output()?;
        assert!(output.status.success());
        assert_list_ids(&String::from_utf8(output.stdout)?, &expected);
    }
    let listed = fixture
        .database
        .command()
        .args([
            "list",
            "--project",
            "foo-bar",
            "--long",
            "--priority",
            "highest",
        ])
        .output()?;
    assert!(listed.status.success());
    let stdout = String::from_utf8(listed.stdout)?;
    assert_list_ids(&stdout, &["FOO-0003", "FOO-0002"]);
    assert_eq!(stdout.matches("priority: highest").count(), 2);
    assert!(task_json(&fixture.database, &task_id("FOO-0003")?)?["priority"].is_null());

    fixture.database.write_user_config("")?;
    let output = fixture.database.command().args(["foo-bar"]).output()?;
    assert!(output.status.success());
    assert_list_ids(
        &String::from_utf8(output.stdout)?,
        &["FOO-0003", "FOO-0002", "FOO-0001"],
    );
    Ok(())
}

fn assert_list_ids(output: &str, expected: &[&str]) {
    let actual: Vec<_> = output
        .lines()
        .filter_map(|line| {
            line.split_whitespace()
                .next()
                .filter(|word| word.starts_with("FOO-"))
        })
        .collect();
    assert_eq!(actual, expected, "{output}");
}

#[test]
fn list_order_help_and_invalid_values_are_cli_contracts() {
    let output = command().args(["task", "list", "--help"]).output().unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for word in ["priority", "effort", "title", "default_sort_order"] {
        assert!(help.contains(word), "{help}");
    }
    for value in ["priority:up", "effort:", "title:asc:desc"] {
        command()
            .args(["list", "--order", value])
            .assert()
            .failure()
            .stdout("");
    }
}

#[test]
fn task_mutations_print_one_summary_line() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    for (args, expected) in [
        (
            vec![
                "task",
                "add",
                "foo-bar",
                "--title",
                "first title",
                "--goal",
                "exercise confirmations",
            ],
            "Added task: FOO-0001 :: first title\n",
        ),
        (
            vec!["task", "edit", "FOO-0001", "--title", "edited title"],
            "Edited task: FOO-0001 :: edited title\n",
        ),
        (
            vec!["task", "done", "FOO-0001"],
            "Done task: FOO-0001 :: edited title\n",
        ),
        (
            vec!["task", "reopen", "FOO-0001", "--yes"],
            "Reopened task: FOO-0001 :: edited title\n",
        ),
        (
            vec!["task", "reopen", "FOO-0001", "--yes"],
            "Skipped task: FOO-0001 :: edited title\n",
        ),
        (
            vec!["task", "cancel", "FOO-0001", "--report", "no longer needed"],
            "Cancelled task: FOO-0001 :: edited title\n",
        ),
        (
            vec!["task", "remove", "FOO-0001", "--yes"],
            "Removed task: FOO-0001 :: edited title\n",
        ),
    ] {
        fixture
            .database
            .command()
            .args(&args)
            .assert()
            .success()
            .stdout(expected);
    }
}

#[test]
#[cfg(unix)]
fn task_mutations_use_configured_lifecycle_colors() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .write_user_config(
            "[colors.task]\nactive = \"#010203\"\ndone = \"#040506\"\ncancelled = \"#070809\"\n",
        )
        .unwrap();
    for (args, verb, color) in [
        (
            vec![
                "task",
                "add",
                "foo-bar",
                "--title",
                "colored task",
                "--goal",
                "exercise confirmations",
            ],
            "Added",
            color_rgb(1, 2, 3),
        ),
        (
            vec![
                "task",
                "edit",
                "FOO-0001",
                "--add-goal",
                "preserve the title",
            ],
            "Edited",
            color_rgb(1, 2, 3),
        ),
        (vec!["task", "done", "FOO-0001"], "Done", color_rgb(4, 5, 6)),
        (
            vec!["task", "reopen", "FOO-0001", "--yes"],
            "Reopened",
            color_rgb(1, 2, 3),
        ),
        (
            vec!["task", "cancel", "FOO-0001", "--report", "no longer needed"],
            "Cancelled",
            color_rgb(7, 8, 9),
        ),
        (
            vec!["task", "remove", "FOO-0001", "--yes"],
            "Removed",
            color_rgb(7, 8, 9),
        ),
    ] {
        let expected = format!("{verb} task: {color}FOO-0001{color:#} :: colored task\n");
        fixture
            .database
            .command()
            .color()
            .args(&args)
            .assert()
            .success()
            .stdout(expected);
    }
}
