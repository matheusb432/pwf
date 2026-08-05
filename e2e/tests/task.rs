use assert_cmd::prelude::OutputAssertExt as _;
#[cfg(target_os = "linux")]
use expectrl::Expect;
use serde_json::{Value, json};

use crate::shared::{ManagedProject, project_id, task_id, task_json};

#[test]
fn machine_add_maps_each_explicit_value_without_parsing_lane_markers() {
    let fixture = ManagedProject::new(project_id("FOO"), "foo-bar");

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

    let task = task_json(&fixture.database, &task_id("FOO-0001"));
    assert_eq!(task["title"], "ship parser");
    assert_eq!(
        task["prompt"],
        "## Goals\n\n- preserve the authored goal\n- keep /d as literal text\n\n## Context\n\n- the machine supplies independent values\n\n## Constraints\n\n- preserve shorthand mode\n\n## Done When\n\n- both input modes are covered"
    );
}

#[test]
fn root_edit_replaces_lane_collections_with_explicit_remove_then_add_actions() {
    let fixture = ManagedProject::new(project_id("FOO"), "foo-bar");
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

    let task = task_json(&fixture.database, &task_id("FOO-0001"));
    assert_eq!(task["title"], "edited task");
    assert_eq!(
        task["prompt"],
        "## Goals\n\n- new goal /c stays literal\n\n## Context\n\n- new context\n\n## Constraints\n\n- new constraint\n\n## Done When\n\n- new outcome"
    );
}

#[test]
fn task_help_exposes_only_the_supported_add_and_edit_contract() {
    let fixture = ManagedProject::new(project_id("FOO"), "foo-bar");
    let add = fixture
        .database
        .command()
        .args(["task", "add", "--help"])
        .output()
        .unwrap();
    assert!(add.status.success());
    let add_help = String::from_utf8(add.stdout).unwrap();
    for flag in [
        "--title",
        "--goal",
        "--context",
        "--constraint",
        "--done-when",
    ] {
        assert!(add_help.contains(flag), "missing {flag}:\n{add_help}");
    }
    for retired in ["--continue", "--section", "--date"] {
        assert!(!add_help.contains(retired), "found {retired}:\n{add_help}");
    }

    let edit = fixture
        .database
        .command()
        .args(["task", "edit", "--help"])
        .output()
        .unwrap();
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
        "--add-prereq",
        "--remove-prereqs",
        "--add-tag",
        "--remove-tags",
        "--remove-effort",
    ] {
        assert!(edit_help.contains(flag), "missing {flag}:\n{edit_help}");
    }
    for retired in [
        "--clear-prereq",
        "--tags-clear",
        "--append-report",
        "--commits",
        "--date",
    ] {
        assert!(
            !edit_help.contains(retired),
            "found {retired}:\n{edit_help}"
        );
    }
}

#[test]
fn shorthand_and_machine_lane_inputs_conflict_before_mutation() {
    let fixture = ManagedProject::new(project_id("FOO"), "foo-bar");
    fixture
        .database
        .command()
        .args(["add", "foo-bar", "original / keep this goal"])
        .assert()
        .success();
    let before = task_json(&fixture.database, &task_id("FOO-0001"));

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

    assert_eq!(task_json(&fixture.database, &task_id("FOO-0001")), before);
}

#[test]
fn edit_rejects_closed_tasks_without_mutating_completion_data() {
    let fixture = ManagedProject::new(project_id("FOO"), "foo-bar");
    fixture
        .database
        .command()
        .args(["add", "foo-bar", "closed task / keep this goal"])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args(["done", "FOO-0001", "--commits", "a..b"])
        .assert()
        .success();
    let before = task_json(&fixture.database, &task_id("FOO-0001"));

    let output = fixture
        .database
        .command()
        .args(["edit", "FOO-0001", "--title", "changed"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "Error: cannot edit closed task FOO-0001.\n"
    );
    assert_eq!(task_json(&fixture.database, &task_id("FOO-0001")), before);
}

#[test]
fn retired_task_date_and_update_surfaces_are_rejected() {
    let fixture = ManagedProject::new(project_id("FOO"), "foo-bar");
    fixture
        .database
        .command()
        .args([
            "task",
            "add",
            "foo-bar",
            "--title",
            "dated task",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .failure();
    fixture
        .database
        .command()
        .args(["task", "update", "FOO-0001", "--title", "changed"])
        .assert()
        .failure();
    fixture
        .database
        .command()
        .args(["update", "FOO-0001", "--title", "changed"])
        .assert()
        .failure();
    fixture
        .database
        .command()
        .args(["show", "FOO-0001", "--json"])
        .assert()
        .failure();
}

#[test]
fn add_stores_the_title_separately_from_goals() {
    let fixture = ManagedProject::new(project_id("FOO"), "foo-bar");

    fixture
        .database
        .command()
        .args(["add", "foo-bar", "ship parser / preserve the authored goal"])
        .assert()
        .success();

    let task = task_json(&fixture.database, &task_id("FOO-0001"));
    assert_eq!(task["title"], "ship parser");
    assert_eq!(task["prompt"], "## Goals\n\n- preserve the authored goal");
}

#[test]
fn add_rejects_an_inferred_title_over_200_characters_without_creating_a_task() {
    let fixture = ManagedProject::new(project_id("FOO"), "foo-bar");
    let title = "\u{e9}".repeat(201);

    let output = fixture
        .database
        .command()
        .args(["add", "foo-bar", &title])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "Error: TaskTitle is too long: the maximum valid length is 200 characters.\n"
    );
    fixture
        .database
        .command()
        .args(["show", "FOO-0001", "--json"])
        .assert()
        .failure();
}

#[test]
fn edit_rejects_a_title_over_200_characters_without_mutating_the_task() {
    let fixture = ManagedProject::new(project_id("FOO"), "foo-bar");
    fixture
        .database
        .command()
        .args(["add", "foo-bar", "keep this title / keep this goal"])
        .assert()
        .success();
    let title = "\u{e9}".repeat(201);

    let output = fixture
        .database
        .command()
        .args(["edit", "FOO-0001", "--title", &title])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "Error: TaskTitle is too long: the maximum valid length is 200 characters.\n"
    );
    assert_eq!(
        task_json(&fixture.database, &task_id("FOO-0001"))["title"],
        "keep this title"
    );
}

#[test]
fn list_status_and_review_task_compose_across_commands() {
    let fixture = ManagedProject::new(project_id("FOO"), "foo-bar");
    fixture
        .database
        .command()
        .args([
            "add",
            "foo-bar",
            "--title",
            "implementation",
            "--goal",
            "implementation work",
        ])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args(["done", "FOO-0001", "--commits", "a..b", "--review"])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args([
            "add",
            "foo-bar",
            "--title",
            "cancelled",
            "--goal",
            "cancelled work",
        ])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args(["cancel", "FOO-0003", "--report", "superseded"])
        .assert()
        .success();

    let review = task_json(&fixture.database, &task_id("FOO-0002"));
    assert_eq!(review["status"], "active");
    assert_eq!(review["section"], "Human");
    assert_eq!(review["title"], "review foo-0001, commits; a..b");
    assert_eq!(
        review["prompt"],
        "## Goals\n\n- git-tools diff a..b\n- git-tools diff-subrepos"
    );

    for (status, present, absent) in [
        ("active", "FOO-0002", ["FOO-0001", "FOO-0003"]),
        ("done", "FOO-0001", ["FOO-0002", "FOO-0003"]),
        ("cancelled", "FOO-0003", ["FOO-0001", "FOO-0002"]),
    ] {
        let output = fixture
            .database
            .command()
            .args(["list", "--status", status, "--all"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains(present), "{status} list: {stdout}");
        for id in absent {
            assert!(!stdout.contains(id), "{status} list leaked {id}: {stdout}");
        }
    }
}

#[test]
#[cfg(target_os = "linux")]
fn remove_prompt_identifies_closed_status_before_deletion() {
    let fixture = ManagedProject::new(project_id("FOO"), "foo-bar");
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
    session.expect("FOO-0001 :: completed work (done)").unwrap();
    session.expect("[Y/n]").unwrap();
    session.send_line("y").unwrap();
    session.expect(expectrl::Eof).unwrap();
    assert!(matches!(
        session.get_process().wait().unwrap(),
        expectrl::process::unix::WaitStatus::Exited(_, 0)
    ));

    fixture
        .database
        .command()
        .args(["show", "FOO-0001", "--json"])
        .assert()
        .failure();
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one isolated public-command lifecycle owns all state transitions"
)]
fn lifecycle_is_observable_through_show_json() {
    let fixture = ManagedProject::new(project_id("FOO"), "foo-bar");
    fixture
        .database
        .command()
        .args([
            "add",
            "foo-bar",
            "--title",
            "prerequisite",
            "--goal",
            "prerequisite work",
        ])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args([
            "add",
            "foo-bar",
            "--title",
            "just done",
            "--goal",
            "complete the work",
            "--prereq",
            "FOO-0001",
            "--effort",
            "high",
            "--tag",
            "cli",
            "--tag",
            "sqlite",
        ])
        .assert()
        .success();

    let active = task_json(&fixture.database, &task_id("FOO-0002"));
    assert_eq!(active["id"], "FOO-0002");
    assert_eq!(active["project"], "foo-bar");
    assert_eq!(active["title"], "just done");
    assert_eq!(active["status"], "active");
    assert!(active["created"].as_str().is_some());
    assert_eq!(active["tags"], json!(["cli", "sqlite"]));
    assert_eq!(active["effort"], "high");
    assert_eq!(active["prerequisites"], json!(["FOO-0001"]));
    assert_eq!(active["section"], Value::Null);
    assert_eq!(active["prompt"], "## Goals\n\n- complete the work");

    fixture
        .database
        .command()
        .args([
            "edit",
            "FOO-0002",
            "--prompt",
            "ship it / preserve the revised goal",
            "--remove-tags",
            "--add-tag",
            "rust",
            "--remove-prereqs",
            "--remove-effort",
        ])
        .assert()
        .success();
    let updated = task_json(&fixture.database, &task_id("FOO-0002"));
    assert_eq!(updated["title"], "ship it");
    assert_eq!(updated["tags"], json!(["rust"]));
    assert_eq!(updated["effort"], Value::Null);
    assert_eq!(updated["prerequisites"], Value::Null);
    assert_eq!(updated["prompt"], "## Goals\n\n- preserve the revised goal");

    fixture
        .database
        .command()
        .args(["done", "FOO-0002", "--commits", "a..b"])
        .assert()
        .success();
    let done = task_json(&fixture.database, &task_id("FOO-0002"));
    assert_eq!(done["status"], "done");
    assert!(done["completed"].as_str().is_some());
    assert_eq!(done["commits"], "a..b");

    fixture
        .database
        .command()
        .args(["reopen", "FOO-0002"])
        .assert()
        .success();
    let reopened = task_json(&fixture.database, &task_id("FOO-0002"));
    assert_eq!(reopened["status"], "active");
    assert_eq!(reopened["completed"], Value::Null);
    assert_eq!(reopened["commits"], Value::Null);
    assert_eq!(reopened["prerequisites"], Value::Null);
}
