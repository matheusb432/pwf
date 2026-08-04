use assert_cmd::prelude::OutputAssertExt as _;
#[cfg(target_os = "linux")]
use expectrl::Expect;
use serde_json::{Value, json};

use crate::shared::{ManagedProject, project_id, task_id, task_json};

#[test]
fn add_stores_the_title_separately_from_goals() {
    let fixture = ManagedProject::new(project_id("FOO"), "foo-bar");

    fixture
        .database
        .command()
        .args([
            "add",
            "foo-bar",
            "ship parser / preserve the authored goal",
            "--date",
            "2026-07-29",
        ])
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
fn update_rejects_a_title_over_200_characters_without_mutating_the_task() {
    let fixture = ManagedProject::new(project_id("FOO"), "foo-bar");
    fixture
        .database
        .command()
        .args([
            "add",
            "foo-bar",
            "keep this title / keep this goal",
            "--date",
            "2026-07-29",
        ])
        .assert()
        .success();
    let title = "\u{e9}".repeat(201);

    let output = fixture
        .database
        .command()
        .args(["update", "FOO-0001", "--title", &title])
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
            "implementation work",
            "--title",
            "implementation",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args([
            "done",
            "FOO-0001",
            "--commits",
            "a..b",
            "--review",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args([
            "add",
            "foo-bar",
            "cancelled work",
            "--title",
            "cancelled",
            "--date",
            "2026-01-02",
        ])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args([
            "cancel",
            "FOO-0003",
            "--report",
            "superseded",
            "--date",
            "2026-01-03",
        ])
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
            "remove completed work",
            "--title",
            "completed work",
            "--date",
            "2026-01-01",
        ])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args(["done", "FOO-0001", "--date", "2026-01-02"])
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
            "prerequisite work",
            "--title",
            "prerequisite",
            "--date",
            "2026-06-19",
        ])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args([
            "add",
            "foo-bar",
            "finish it / complete the work",
            "--title",
            "just done",
            "--date",
            "2026-06-20",
            "--section",
            "future",
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
    assert_eq!(active["created"], "2026-06-20");
    assert_eq!(active["tags"], json!(["cli", "sqlite"]));
    assert_eq!(active["effort"], "high");
    assert_eq!(active["prerequisites"], json!(["FOO-0001"]));
    assert_eq!(active["section"], "Future");
    assert_eq!(active["prompt"], "## Goals\n\n- complete the work");

    fixture
        .database
        .command()
        .args([
            "update",
            "FOO-0002",
            "--title",
            "ship it",
            "--prompt",
            "revised work / preserve the revised goal",
            "--tag",
            "rust",
            "--clear-prereq",
        ])
        .assert()
        .success();
    let updated = task_json(&fixture.database, &task_id("FOO-0002"));
    assert_eq!(updated["title"], "ship it");
    assert_eq!(updated["tags"], json!(["cli", "sqlite", "rust"]));
    assert_eq!(updated["prerequisites"], Value::Null);
    assert_eq!(updated["prompt"], "## Goals\n\n- preserve the revised goal");

    fixture
        .database
        .command()
        .args([
            "done",
            "FOO-0002",
            "--date",
            "2026-06-21",
            "--commits",
            "a..b",
        ])
        .assert()
        .success();
    let done = task_json(&fixture.database, &task_id("FOO-0002"));
    assert_eq!(done["status"], "done");
    assert_eq!(done["completed"], "2026-06-21");
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
