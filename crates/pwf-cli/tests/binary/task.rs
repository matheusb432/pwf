use assert_cmd::prelude::OutputAssertExt as _;
#[cfg(target_os = "linux")]
use expectrl::Expect;

use crate::support::{
    CommandTestExt, ManagedProject, ProjectFixture, command, project_id,
    style::{assert_plain, color_rgb},
    task_id, task_json,
};

#[test]
fn markdown_body_add_preserves_content_and_treats_the_positional_text_as_title() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    let markdown = "---\n\n## Request\r\n\r\nKeep /g literal and `src/main.rs`.  \r\n\r\n| Input | Result |\r\n| --- | --- |\r\n| /c | unchanged |\r\n\r\n```sh\r\nprintf '%s' '$value'\r\n```\r\n\r\n";
    fixture
        .database
        .command()
        .args(["add", "foo", " \t ", "--body", markdown])
        .assert()
        .failure()
        .code(1)
        .stdout("");
    for (number, body) in [(1, markdown), (2, "")] {
        fixture
            .database
            .command()
            .args([
                "task",
                "add",
                "foo",
                "API / CLI /g documentation",
                "--body",
                body,
                "--tag",
                "docs",
                "--effort",
                "low",
                "--priority",
                "high",
            ])
            .assert()
            .success()
            .stderr("");

        let id = task_id(&format!("FOO-{number:04}")).unwrap();
        let task = task_json(&fixture.database, &id).unwrap();
        assert_eq!(task["title"], "API / CLI /g documentation");
        assert_eq!(task["tags"], serde_json::json!(["docs"]));
        assert_eq!(task["effort"], "low");
        assert_eq!(task["priority"], "high");
        let path = fixture
            .database
            .command_args(&["task", "get", id.as_ref(), "--path"])
            .success_stdout();
        let stored = std::fs::read_to_string(path.trim()).unwrap();
        let (_, stored_body) = stored.split_once("\n---\n").unwrap();
        assert_eq!(stored_body, body);
    }
}

#[test]
fn markdown_body_edit_preserves_metadata_and_supports_title_edits_and_clearing() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args([
            "task",
            "add",
            "foo",
            "original / old goal",
            "--tag",
            "docs",
            "--effort",
            "low",
            "--priority",
            "high",
        ])
        .assert()
        .success();
    let path = fixture
        .database
        .command_args(&["task", "get", "FOO-0001", "--path"])
        .success_stdout();
    let path = path.trim();
    let before = std::fs::read_to_string(path).unwrap();
    let (frontmatter, _) = before.split_once("\n---\n").unwrap();
    let markdown = "---\n\n## Replacement\n\nKeep /g and /c as text.  \n\n";

    fixture
        .database
        .command()
        .args(["task", "edit", "FOO-0001", "--body", markdown])
        .assert()
        .success()
        .stderr("");
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        format!("{frontmatter}\n---\n{markdown}")
    );

    fixture
        .database
        .command()
        .args([
            "task",
            "edit",
            "FOO-0001",
            "--body",
            "# New body without a final newline",
            "--title",
            "new /g title",
        ])
        .assert()
        .success()
        .stderr("");
    let task = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();
    assert_eq!(task["title"], "new /g title");
    assert_eq!(task["body"], "# New body without a final newline");
    let stored = std::fs::read_to_string(path).unwrap();
    let (frontmatter, body) = stored.split_once("\n---\n").unwrap();
    assert_eq!(body, "# New body without a final newline");

    fixture
        .database
        .command()
        .args(["edit", "FOO-0001", "--body", ""])
        .assert()
        .success()
        .stderr("");
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        format!("{frontmatter}\n---\n")
    );
}

#[test]
fn from_file_add_uses_file_stem_as_title_and_preserves_content() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    let source_directory = tempfile::tempdir().unwrap();
    let source_path = source_directory.path().join("My very cool task.md");
    let source = "# This is important\n\nKeep /g and all authored formatting.\n";

    let missing = fixture
        .database
        .command()
        .args(["task", "add", "from-file"])
        .arg(&source_path)
        .args(["--project", "foo"])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert_eq!(missing.stdout, b"");
    let stderr = String::from_utf8(missing.stderr).unwrap();
    assert!(stderr.contains("cannot read task source file"), "{stderr}");

    std::fs::write(&source_path, source).unwrap();

    fixture
        .database
        .command()
        .args(["task", "add", "from-file"])
        .arg(&source_path)
        .args(["--project", "foo"])
        .assert()
        .success()
        .stderr("");

    let task = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();
    assert_eq!(task["title"], "My very cool task");
    assert_eq!(task["body"], source.trim_end());
    let task_path = fixture
        .database
        .command_args(&["task", "get", "FOO-0001", "--path"])
        .success_stdout();
    let stored = std::fs::read_to_string(task_path.trim()).unwrap();
    assert!(stored.ends_with(source), "{stored:?}");
}

#[test]
fn add_and_edit_short_options_persist_task_metadata() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    for title in ["first blocker", "second blocker"] {
        fixture
            .database
            .command()
            .args(["task", "add", "foo", title])
            .assert()
            .success();
    }

    fixture
        .database
        .command()
        .args([
            "task",
            "add",
            "foo",
            "exercise short options",
            "-b",
            "[[FOO-0001]]",
            "-t",
            "rust",
            "-e",
            "high",
            "-p",
            "low",
        ])
        .assert()
        .success()
        .stderr("");

    let id = task_id("FOO-0003").unwrap();
    let added = task_json(&fixture.database, &id).unwrap();
    assert_eq!(added["blocked_by"], serde_json::json!(["FOO-0001"]));
    assert_eq!(added["tags"], serde_json::json!(["rust"]));
    assert_eq!(added["effort"], "high");
    assert_eq!(added["priority"], "low");

    fixture
        .database
        .command()
        .args([
            "task", "edit", "FOO-0003", "-b", "FOO-0002", "-t", "sqlite", "-e", "medium", "-p",
            "highest",
        ])
        .assert()
        .success()
        .stderr("");

    let edited = task_json(&fixture.database, &id).unwrap();
    assert_eq!(
        edited["blocked_by"],
        serde_json::json!(["FOO-0001", "FOO-0002"])
    );
    assert_eq!(edited["tags"], serde_json::json!(["rust", "sqlite"]));
    assert_eq!(edited["effort"], "medium");
    assert_eq!(edited["priority"], "highest");
}

#[test]
fn shorthand_add_renders_the_project_preset_and_sections_describes_it() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .write_user_config(concat!(
            "[task_body.projects]\nfoo = \"prompt\"\n",
            "[task_body.presets.prompt]\nsections = [\n",
            "  { marker = \"/g\", title = \"Goals\", level = 3, items = \"numbered\" },\n",
            "  { marker = \"/c\", title = \"Context\", level = 4, items = \"paragraph\" },\n",
            "]\n",
        ))
        .unwrap();

    fixture
        .database
        .command()
        .args([
            "task",
            "add",
            "foo",
            "Web: UI fixes; keep #123 and \"quotes\" / first goal / second goal /c why it matters / what exists",
        ])
        .assert()
        .success()
        .stderr("");

    let task = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();
    assert_eq!(task["title"], "Web: UI fixes; keep #123 and \"quotes\"");
    assert_eq!(
        task["body"],
        "### Goals\n\n1. first goal\n2. second goal\n\n#### Context\n\nwhy it matters\n\nwhat exists"
    );

    fixture
        .database
        .command()
        .args(["task", "sections", "foo"])
        .assert()
        .success()
        .stdout(concat!(
            "Preset: prompt\n\n",
            "/g  ### Goals     numbered\n",
            "/c  #### Context  paragraph\n\n",
            "Text before the first marker is the title. `/` starts another item in the current section; text after it goes to the first section until a marker selects another.\n",
        ))
        .stderr("");
    let global = fixture
        .database
        .command()
        .args(["task", "sections", "--json"])
        .output()
        .unwrap();
    assert!(global.status.success());
    let global: serde_json::Value = serde_json::from_slice(&global.stdout).unwrap();
    assert_eq!(global["preset"], "default");
    assert_eq!(
        global["sections"][3],
        serde_json::json!({
            "marker": "/d",
            "header": "Done When",
            "heading_level": 2,
            "item_style": "bullet",
        })
    );

    let missing = fixture
        .database
        .command()
        .args(["task", "sections", "miss"])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert_eq!(missing.stdout, b"");
    assert!(
        String::from_utf8(missing.stderr).unwrap().contains("MISS"),
        "missing project"
    );
}

#[test]
fn shorthand_add_preserves_explicit_empty_marker_sections() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();

    fixture
        .database
        .command()
        .args(["add", "foo", "my task /c some context /d /c"])
        .assert()
        .success()
        .stderr("");

    let task = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();
    assert_eq!(
        task["body"],
        "## Goals\n\n\n## Context\n\n- some context\n\n## Done When"
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
                "foo",
                format!("{title} / exercise priority listing").as_str(),
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
            "foo",
            "--all",
            "--long=rich",
            "--priority",
            "highest",
        ])
        .output()
        .unwrap();

    assert!(listed.status.success());
    let output = String::from_utf8(listed.stdout).unwrap();
    assert!(output.contains("FOO-0002"), "{output}");
    assert!(!output.contains("FOO-0001"), "{output}");
    assert!(output.contains("Priority  highest"), "{output}");
}

#[test]
#[cfg(unix)]
fn colored_all_status_list_uses_color_instead_of_a_status_tag() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args(["add", "foo", "orange task / render the configured color"])
        .assert()
        .success();
    fixture
        .database
        .write_user_config("[colors.task]\nactive = \"#ff8700\"\n")
        .unwrap();

    let plain = fixture
        .database
        .command()
        .args(["list", "--project", "foo", "--all"])
        .output()
        .unwrap();
    assert!(plain.status.success());
    let stdout = String::from_utf8(plain.stdout).unwrap();
    assert!(stdout.contains("FOO-0001"), "{stdout:?}");
    assert!(stdout.contains("[active]"), "{stdout:?}");
    assert!(stdout.contains("orange task"), "{stdout:?}");
    assert_plain(&stdout);

    for arguments in [
        ["foo"].as_slice(),
        ["list", "--project", "foo", "--all"].as_slice(),
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
                "{orange}FOO-0001 {orange:#}",
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

    let output = fixture.database.command().arg("foo").output().unwrap();

    assert!(!output.status.success());
    assert_eq!(output.stdout, b"");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(&*config_path.to_string_lossy()), "{stderr}");
    assert!(
        stderr.contains("`colors.task.active` is invalid"),
        "{stderr}"
    );
    assert!(stderr.contains("#RRGGBB"), "{stderr}");
}

#[test]
fn root_edit_replaces_or_appends_shorthand_body_content() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args([
            "task",
            "add",
            "foo",
            "original task / old goal /c old context /n old constraint /d old outcome",
        ])
        .assert()
        .success();

    fixture
        .database
        .command()
        .args([
            "edit",
            "FOO-0001",
            "--replace-body",
            "[WIP]: Edited task; keep #456 / new goal /d new outcome",
        ])
        .assert()
        .success();
    let task = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();
    assert_eq!(task["title"], "[WIP]: Edited task; keep #456");
    assert_eq!(
        task["body"],
        "## Goals\n\n- new goal\n\n## Done When\n\n- new outcome"
    );

    fixture
        .database
        .command()
        .args([
            "edit",
            "FOO-0001",
            "-a",
            "second goal /c some new context",
            "--title",
            "appended task",
        ])
        .assert()
        .success();
    let task = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();
    assert_eq!(task["title"], "appended task");
    assert_eq!(
        task["body"],
        "## Goals\n\n- new goal\n- second goal\n\n## Done When\n\n- new outcome\n\n## Context\n\n- some new context"
    );
}

#[test]
fn removed_section_workflow_flags_are_rejected_by_the_parser() {
    for arguments in [
        vec!["task", "add", "foo", "ship it", "--human"],
        vec!["task", "done", "FOO-0001", "--review"],
        vec!["task", "cancel", "FOO-0001", "-r", "obsolete", "--review"],
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
fn task_list_rejects_section_before_connecting() {
    for arguments in [
        vec!["task", "list", "--section", "Waiting on API"],
        vec!["task", "list", "--section", "Waiting on API", "--all"],
        vec!["list", "--section", "Waiting on API"],
    ] {
        let output = command().args(&arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert_eq!(output.stdout, b"");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.contains("unexpected argument '--section'"),
            "{stderr}"
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
            "foo",
            "prepare graph data / supply the dependency",
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
            "foo",
            "cancel obsolete renderer / preserve cancelled task output",
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
            "-r",
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
            "foo",
            "render graph view / show the dependency",
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
fn invalid_argument_combinations_fail_before_mutation() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args(["add", "foo", "original / keep this goal"])
        .assert()
        .success();
    let before = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();

    for arguments in [
        vec![
            "edit",
            "FOO-0001",
            "--replace-body",
            "replacement / goal",
            "--append-body",
            "ambiguous",
        ],
        vec![
            "edit",
            "FOO-0001",
            "--replace-body",
            "replacement / goal",
            "--title",
            "ambiguous",
        ],
        vec!["edit", "FOO-0001", "--add-goal", "removed flag"],
        vec![
            "edit",
            "FOO-0001",
            "--body",
            "# Markdown",
            "--append-body",
            "ambiguous",
        ],
        vec![
            "edit",
            "FOO-0001",
            "--body",
            "# Markdown",
            "--replace-body",
            "ambiguous",
        ],
        vec!["add", "foo", "--body", "# Missing title"],
        vec!["edit", "FOO-0001", "--effort", "high", "--remove-effort"],
        vec![
            "edit",
            "FOO-0001",
            "--priority",
            "high",
            "--remove-priority",
        ],
        vec!["edit", "FOO-0001"],
        vec!["task", "clone"],
        vec!["task", "get"],
        vec!["task", "add", "foo", "--title", "removed flag"],
        vec!["add", "foo", "--goal", "removed flag"],
        vec!["note", "edit", "foo", "1"],
        vec![
            "note",
            "edit",
            "foo",
            "1",
            "--domain",
            "docs",
            "--remove-domain",
        ],
        vec![
            "note",
            "add",
            "foo",
            "title / body",
            "--title",
            "explicit",
            "--content",
            "body",
        ],
        vec!["project", "edit", "FOO"],
        vec![
            "project",
            "edit",
            "FOO",
            "--source",
            "/tmp",
            "--clear-source",
        ],
    ] {
        fixture
            .database
            .command()
            .args(arguments)
            .assert()
            .failure()
            .code(2);
    }

    assert_eq!(
        task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap(),
        before
    );
}

#[test]
#[cfg(target_os = "linux")]
fn remove_confirmation_identifies_closed_status_before_deletion() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args(["add", "foo", "completed work / remove completed work"])
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
        .args(["get", "FOO-0001", "--long=json"])
        .assert()
        .failure();
}

#[test]
#[cfg(unix)]
fn configured_list_page_size_applies_to_every_list_spelling() -> anyhow::Result<()> {
    let fixture = ManagedProject::new(&project_id("FOO")?, "foo-bar")?;
    for title in ["first", "second", "third"] {
        fixture
            .database
            .command()
            .args([
                "add",
                "foo",
                format!("{title} / exercise list limits").as_str(),
            ])
            .assert()
            .success();
    }
    fixture
        .database
        .write_user_config("default_list_page_size = 2\n")?;
    for prefix in [
        vec!["task", "list", "--project", "foo"],
        vec!["list", "--project", "foo"],
        vec!["foo"],
    ] {
        for (options, expected) in [
            (vec![], vec!["FOO-0003", "FOO-0002"]),
            (vec!["--number", "1"], vec!["FOO-0003"]),
            (vec!["--all"], vec!["FOO-0003", "FOO-0002", "FOO-0001"]),
            (vec!["--all", "-n", "1"], vec!["FOO-0003"]),
        ] {
            let output = fixture
                .database
                .command()
                .args(&prefix)
                .args(options)
                .output()?;
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stderr, b"");
            assert_list_ids(&String::from_utf8(output.stdout)?, &expected);
        }
    }
    fixture
        .database
        .write_user_config("default_list_page_size = 1\n")?;
    let output = fixture
        .database
        .command()
        .args(["foo", "--long=json"])
        .output()?;
    assert!(output.status.success());
    let tasks: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout)?;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["id"], "FOO-0003");
    Ok(())
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
        let shorthand = format!("{title} / exercise list ordering");
        let mut arguments = vec!["add", "foo", shorthand.as_str()];
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
        vec!["task", "list", "--project", "foo"],
        vec!["list", "--project", "foo"],
        vec!["foo"],
    ] {
        let output = fixture.database.command().args(&prefix).output()?;
        assert!(output.status.success());
        assert_eq!(output.stderr, b"");
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
            .args(["list", "--project", "foo", "--order", order])
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
            "foo",
            "--long=rich",
            "--priority",
            "highest",
        ])
        .output()?;
    assert!(listed.status.success());
    let stdout = String::from_utf8(listed.stdout)?;
    assert_list_ids(&stdout, &["FOO-0003", "FOO-0002"]);
    assert_eq!(stdout.matches("Priority  highest").count(), 2);
    assert!(task_json(&fixture.database, &task_id("FOO-0003")?)?["priority"].is_null());

    fixture.database.write_user_config("")?;
    let output = fixture.database.command().args(["foo"]).output()?;
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
fn task_mutations_print_one_summary_line() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    for (args, expected) in [
        (
            vec!["task", "add", "foo", "first title / exercise confirmations"],
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
            vec!["task", "activate", "FOO-0001", "--yes"],
            "Activated task: FOO-0001 :: edited title\n",
        ),
        (
            vec!["task", "activate", "FOO-0001", "--yes"],
            "Already active task: FOO-0001 :: edited title\n",
        ),
        (
            vec!["task", "cancel", "FOO-0001", "-r", "no longer needed"],
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

#[cfg(unix)]
#[test]
fn personal_separator_applies_to_task_output_and_reloads_without_changing_authored_titles()
-> anyhow::Result<()> {
    let fixture = ManagedProject::new(&project_id("FOO")?, "foo-bar")?;
    fixture
        .database
        .write_title_config("[task]\nseparator = \" \"\n")?;
    fixture
        .database
        .command()
        .args([
            "task",
            "add",
            "foo",
            "keep :: authored / exercise title formatting",
        ])
        .assert()
        .success()
        .stdout("Added task: FOO-0001 keep :: authored\n");
    for (args, expected) in [
        (
            vec!["task", "list", "--project", "foo"],
            "FOO-0001  keep :: authored\n",
        ),
        (
            vec!["task", "list", "--project", "foo", "--all"],
            "FOO-0001  [active] keep :: authored\n",
        ),
        (
            vec!["task", "edit", "foo1", "--title", "keep :: authored"],
            "Edited task: FOO-0001 keep :: authored\n",
        ),
    ] {
        fixture
            .database
            .command()
            .args(args)
            .assert()
            .success()
            .stdout(expected);
    }
    let rich = fixture
        .database
        .command_args(&["task", "get", "foo1", "--long=rich"])
        .success_stdout();
    assert!(rich.starts_with("FOO-0001 keep :: authored\n\n"), "{rich}");
    let stored_before = task_json(&fixture.database, &task_id("FOO-0001")?)?;
    fixture
        .database
        .write_title_config("[task]\nseparator = \"\"\n")?;
    fixture
        .database
        .command()
        .args(["task", "list", "--project", "foo"])
        .assert()
        .success()
        .stdout("FOO-0001 keep :: authored\n");
    assert_eq!(
        task_json(&fixture.database, &task_id("FOO-0001")?)?,
        stored_before
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn task_mutations_use_configured_lifecycle_colors() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .write_user_config(
            "[colors.task]\nactive = \"#010203\"\ndone = \"#040506\"\ncancelled = \"#070809\"\nbacklog = \"#0a0b0c\"\n",
        )
        .unwrap();
    for (args, verb, color) in [
        (
            vec![
                "task",
                "add",
                "foo",
                "colored task / exercise confirmations",
            ],
            "Added",
            color_rgb(1, 2, 3),
        ),
        (
            vec![
                "task",
                "edit",
                "FOO-0001",
                "--append-body",
                "preserve the title",
            ],
            "Edited",
            color_rgb(1, 2, 3),
        ),
        (
            vec!["task", "backlog", "FOO-0001"],
            "Backlogged",
            color_rgb(10, 11, 12),
        ),
        (
            vec!["task", "activate", "FOO-0001"],
            "Activated",
            color_rgb(1, 2, 3),
        ),
        (vec!["task", "done", "FOO-0001"], "Done", color_rgb(4, 5, 6)),
        (
            vec!["task", "activate", "FOO-0001", "--yes"],
            "Activated",
            color_rgb(1, 2, 3),
        ),
        (
            vec!["task", "cancel", "FOO-0001", "-r", "no longer needed"],
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

#[test]
fn project_ids_ignore_title_collisions_and_exclude_paused_projects() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "alt").unwrap();
    let other = tempfile::tempdir().unwrap();
    let tasks = other.path().join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    fixture.database.add_directory_project(
        &project_id("ALT").unwrap(),
        "other",
        other.path(),
        &tasks,
    );
    for id in ["ALT", "foo"] {
        let output = fixture
            .database
            .command_args(&[
                "task",
                "add",
                id,
                "selected task / use the exact project ID",
            ])
            .success_stdout();
        assert!(
            output.contains(&format!("{}-0001", id.to_ascii_uppercase())),
            "{output}"
        );
    }
    let listed = fixture
        .database
        .command_args(&["task", "list", "--project", "ALT"])
        .success_stdout();
    assert!(listed.contains("ALT-0001"));
    assert!(!listed.contains("FOO-0001"));
    fixture
        .database
        .command_args(&["project", "pause", "FOO", "--json"])
        .success_json();
    let error = fixture
        .database
        .command_args(&["task", "list", "--project", "foo"])
        .output()
        .unwrap();
    assert_eq!(error.status.code(), Some(2));
    assert_eq!(error.stdout, b"");
    let diagnostic = String::from_utf8(error.stderr).unwrap();
    assert!(
        diagnostic.starts_with("error: invalid value 'foo'"),
        "{diagnostic}"
    );
    assert!(!diagnostic.contains("tip:"), "{diagnostic}");
}

#[test]
fn get_formats_the_same_record_as_markdown_path_or_json() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command_args(&["task", "add", "foo", "format task / render in the CLI"])
        .success_stdout();
    let path = fixture
        .database
        .command_args(&["task", "get", "FOO-0001", "--path"])
        .success_stdout();
    let path = std::path::Path::new(path.trim());
    let source = "---\nid: FOO-0001\ntitle: Format Task\nstatus: done\ncreated_at: 2026-07-26T12:34:56Z\ncompleted_at: 2026-08-12T12:34:56Z\neffort: high\npriority: highest\ntags: [rust, sqlite]\n---\n\n  authored body  \n";
    std::fs::write(path, source).unwrap();
    assert_eq!(
        fixture
            .database
            .command_args(&["task", "get", "FOO-0001"])
            .success_stdout(),
        format!("{source}\n")
    );
    let json = fixture
        .database
        .command_args(&["task", "get", "FOO-0001", "--long=json"])
        .success_json();
    assert_eq!(json["project"], "foo-bar");
    assert_eq!(json["title"], "Format Task");
    assert_eq!(json["created"], "2026-07-26");
    assert_eq!(json["completed"], "2026-08-12");
    assert_eq!(json["effort"], "high");
    assert_eq!(json["priority"], "highest");
    assert_eq!(json["tags"], serde_json::json!(["rust", "sqlite"]));
    assert_eq!(json["body"], "authored body");

    let malformed = source.replace("effort: high", "effort: extreme");
    std::fs::write(path, &malformed).unwrap();
    assert_eq!(
        fixture
            .database
            .command_args(&["task", "get", "FOO-0001"])
            .success_stdout(),
        format!("{malformed}\n")
    );
    let rejected = fixture
        .database
        .command_args(&["task", "get", "FOO-0001", "--long=json"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert_eq!(rejected.stdout, b"");
    assert!(
        String::from_utf8(rejected.stderr)
            .unwrap()
            .contains("Invalid task effort")
    );

    std::fs::remove_file(path).unwrap();
    for arguments in [
        vec!["task", "get", "FOO-0001", "--path"],
        vec!["task", "get", "FOO-0001", "--long=json"],
        vec!["task", "get", "FOO-0001"],
    ] {
        let missing = fixture
            .database
            .command()
            .args(&arguments)
            .output()
            .unwrap();
        assert!(!missing.status.success(), "{arguments:?}");
        assert_eq!(missing.stdout, b"");
        let stderr = String::from_utf8(missing.stderr).unwrap();
        assert!(
            stderr.contains("Task not found: FOO-0001"),
            "{arguments:?}: {stderr}"
        );
    }
}

#[test]
fn clone_routes_project_ids_and_preserves_authored_content() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args(["add", "foo", "blocker"])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args([
            "task",
            "add",
            "foo",
            "original task / keep /d literal",
            "--tag",
            "rust",
            "--effort",
            "high",
            "--priority",
            "low",
            "--blocked-by",
            "FOO-0001",
        ])
        .assert()
        .success();
    let original = task_json(&fixture.database, &task_id("FOO-0002").unwrap()).unwrap();
    for arguments in [
        vec!["task", "clone", "foo2"],
        vec!["clone", "--id", "FOO-0002", "--project", "FOO"],
    ] {
        let output = fixture.database.command().args(arguments).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = String::from_utf8(output.stdout).unwrap();
        assert!(output.starts_with("Cloned task: FOO-000"), "{output}");
        assert_eq!(output.lines().count(), 1);
    }
    for id in ["FOO-0003", "FOO-0004"] {
        let cloned = task_json(&fixture.database, &task_id(id).unwrap()).unwrap();
        for field in ["title", "body", "tags", "effort", "priority", "blocked_by"] {
            assert_eq!(cloned[field], original[field], "{id}: {field}");
        }
    }
    assert_eq!(
        task_json(&fixture.database, &task_id("FOO-0002").unwrap()).unwrap(),
        original
    );
}

#[test]
fn task_and_note_crud_preserve_missing_arbitrary_and_malformed_snapshots() -> anyhow::Result<()> {
    for page_source in [
        None,
        Some("# Project notes\n\n## Someday\n- [ ] [[FOO-9999]]\n- [[FOO-NOTE-9999]]\n"),
        Some("---\nid: [broken\n---\n\n- [ ] [[FOO-9999]]\n"),
    ] {
        let directory = tempfile::tempdir()?;
        let project = directory.path().join("project");
        let tasks = directory.path().join("tasks");
        std::fs::create_dir_all(&project)?;
        std::fs::create_dir_all(&tasks)?;
        let page = tasks.join("tasks.md");
        if let Some(source) = page_source {
            std::fs::write(&page, source)?;
        }
        let fixture = ProjectFixture::new()?;
        fixture.add(
            &project_id("FOO")?,
            "foo",
            project.to_str().unwrap(),
            tasks.to_str().unwrap(),
        )?;
        let run = |arguments: &[&str]| -> anyhow::Result<String> {
            let output = fixture.run(arguments)?;
            assert!(
                output.status.success(),
                "{arguments:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                output.stderr.is_empty(),
                "{arguments:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            Ok(String::from_utf8(output.stdout)?)
        };

        for id in ["FOO-0001", "FOO-0002"] {
            let added = run(&[
                "task",
                "add",
                "foo",
                "repeated input / persist each task file",
            ])?;
            assert!(added.contains(id), "{added}");
            assert!(tasks.join(format!("{id}.md")).exists());
        }
        run(&[
            "task",
            "edit",
            "FOO-0001",
            "--title",
            "revised task",
            "--priority",
            "highest",
        ])?;
        let task: serde_json::Value =
            serde_json::from_str(&run(&["task", "get", "FOO-0001", "--long=json"])?)?;
        assert_eq!(task["title"], "revised task");
        assert_eq!(task["priority"], "highest");
        let listed = run(&["task", "list", "--all", "--order", "id:asc"])?;
        assert_list_ids(&listed, &["FOO-0001", "FOO-0002"]);
        assert!(!listed.contains("Someday"), "{listed}");
        run(&["task", "done", "FOO-0001", "-r", "finished"])?;
        let task: serde_json::Value =
            serde_json::from_str(&run(&["task", "get", "FOO-0001", "--long=json"])?)?;
        assert_eq!(task["status"], "done");
        run(&["task", "activate", "FOO-0001", "--yes"])?;
        run(&["task", "cancel", "FOO-0001", "-r", "obsolete"])?;
        let task: serde_json::Value =
            serde_json::from_str(&run(&["task", "get", "FOO-0001", "--long=json"])?)?;
        assert_eq!(task["status"], "cancelled");
        run(&["task", "remove", "FOO-0001", "--yes"])?;
        assert!(!tasks.join("FOO-0001.md").exists());
        let missing = fixture.run(&["task", "get", "FOO-0001"])?;
        assert!(!missing.status.success());
        assert_eq!(missing.stdout, b"");
        assert!(String::from_utf8(missing.stderr)?.contains("FOO-0001"));
        let listed = run(&["task", "list", "--all"])?;
        assert!(!listed.contains("FOO-0001"), "{listed}");
        assert!(listed.contains("FOO-0002"), "{listed}");

        exercise_project_note_crud(&run, &tasks)?;

        match page_source {
            Some(source) => assert_eq!(std::fs::read(&page)?, source.as_bytes()),
            None => assert!(!page.exists()),
        }
    }
    Ok(())
}

fn exercise_project_note_crud(
    run: &impl Fn(&[&str]) -> anyhow::Result<String>,
    tasks: &std::path::Path,
) -> anyhow::Result<()> {
    let added = run(&[
        "note",
        "add",
        "foo",
        "--title",
        "project evidence",
        "--content",
        "authored evidence",
    ])?;
    assert!(added.contains("FOO-NOTE-0001"), "{added}");
    run(&[
        "note",
        "edit",
        "foo",
        "1",
        "--title",
        "revised evidence",
        "--content",
        "new evidence",
    ])?;
    let listed = run(&["note", "list", "foo"])?;
    assert_eq!(listed, "FOO-NOTE-0001 :: revised evidence\n");
    assert!(std::fs::read_to_string(tasks.join("FOO-NOTE-0001.md"))?.contains("new evidence"));
    run(&["note", "remove", "foo", "1", "--yes"])?;
    assert!(!tasks.join("FOO-NOTE-0001.md").exists());
    assert!(!run(&["note", "list", "foo"])?.contains("FOO-NOTE-0001"));
    Ok(())
}

#[test]
fn task_list_all_widens_status_and_cap_without_grouping_snapshot_sections() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let tasks = directory.path().join("tasks");
    std::fs::create_dir_all(&tasks)?;
    let page =
        "# Generated snapshot\n\n## Alpha\n- [ ] [[FOO-0001]]\n\n## Zulu\n- [x] [[FOO-0014]]\n";
    std::fs::write(tasks.join("tasks.md"), page)?;
    for number in 1..=14 {
        let id = format!("FOO-{number:04}");
        let status = match number {
            13 => "done",
            14 => "cancelled",
            _ => "active",
        };
        std::fs::write(
            tasks.join(format!("{id}.md")),
            format!(
                "---\nid: {id}\nstatus: {status}\ntitle: task {number}\nproject: foo\n---\n\n## Goals\n\n- list the actual task file\n"
            ),
        )?;
    }
    let fixture = ProjectFixture::new()?;
    fixture.add(
        &project_id("FOO")?,
        "foo",
        directory.path().to_str().unwrap(),
        tasks.to_str().unwrap(),
    )?;
    let ordered = [
        "FOO-0014", "FOO-0013", "FOO-0012", "FOO-0011", "FOO-0010", "FOO-0009", "FOO-0008",
        "FOO-0007", "FOO-0006", "FOO-0005", "FOO-0004", "FOO-0003", "FOO-0002", "FOO-0001",
    ];
    for (options, expected) in [
        (vec![], &ordered[2..12]),
        (vec!["--all"], ordered.as_slice()),
        (vec!["--all", "--status", "active"], &ordered[2..]),
        (vec!["--all", "-n", "2"], &ordered[..2]),
    ] {
        let mut arguments = vec!["task", "list", "--project", "foo", "--order", "id:desc"];
        arguments.extend(options);
        let output = fixture.run(&arguments)?;
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stderr, b"");
        let output = String::from_utf8(output.stdout)?;
        assert_list_ids(&output, expected);
        assert!(!output.contains("Alpha"), "{output}");
        assert!(!output.contains("Zulu"), "{output}");
    }
    assert_eq!(std::fs::read_to_string(tasks.join("tasks.md"))?, page);
    Ok(())
}

#[test]
fn backlog_is_hidden_by_default_and_retains_its_color_when_edited() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args(["task", "add", "foo", "deferred work / do this later"])
        .assert()
        .success();
    let id = task_id("FOO-0001").unwrap();
    let created = task_json(&fixture.database, &id).unwrap();
    let gold = color_rgb(234, 179, 8);
    fixture
        .database
        .command()
        .color()
        .args(["task", "backlog", "--id", "foo1"])
        .assert()
        .success()
        .stderr("")
        .stdout(format!(
            "Backlogged task: {gold}FOO-0001{gold:#} :: deferred work\n"
        ));
    let backlogged = task_json(&fixture.database, &id).unwrap();
    assert_eq!(backlogged["status"], "backlog");
    assert_eq!(backlogged["created_at"], created["created_at"]);
    assert_eq!(backlogged["completed_at"], serde_json::Value::Null);
    fixture
        .database
        .command()
        .args(["task", "backlog", "FOO-0001"])
        .assert()
        .success()
        .stdout("Already backlogged task: FOO-0001 :: deferred work\n");
    assert_eq!(task_json(&fixture.database, &id).unwrap(), backlogged);

    for args in [
        vec!["task", "list", "--project", "foo"],
        vec!["list", "--project", "foo"],
        vec!["foo"],
    ] {
        let output = fixture.database.command().args(&args).output().unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(
            !String::from_utf8(output.stdout)
                .unwrap()
                .contains("FOO-0001")
        );
        for filter in [vec!["--status", "backlog"], vec!["--all"]] {
            let output = fixture
                .database
                .command()
                .args(&args)
                .args(&filter)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?} {filter:?}: {output:?}");
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert!(stdout.contains("FOO-0001"), "{stdout}");
            assert_plain(&stdout);
        }
    }
    fixture
        .database
        .command()
        .color()
        .args(["task", "edit", "FOO-0001", "--title", "ready later"])
        .assert()
        .success()
        .stdout(format!(
            "Edited task: {gold}FOO-0001{gold:#} :: ready later\n"
        ));
    let dag = fixture
        .database
        .command()
        .color()
        .args(["task", "dag", "FOO-0001", "--status", "backlog"])
        .output()
        .unwrap();
    assert!(dag.status.success(), "{dag:?}");
    assert!(
        String::from_utf8(dag.stdout)
            .unwrap()
            .contains(&format!("{gold}FOO-0001{gold:#}"))
    );
}

#[test]
fn activate_from_backlog_and_already_active_need_no_confirmation() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args(["task", "add", "foo", "ready later / work later"])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args(["task", "backlog", "FOO-0001"])
        .assert()
        .success();
    let id = task_id("FOO-0001").unwrap();
    fixture
        .database
        .command()
        .args(["task", "activate", "foo1"])
        .assert()
        .success()
        .stderr("")
        .stdout("Activated task: FOO-0001 :: ready later\n");
    let active = task_json(&fixture.database, &id).unwrap();
    assert_eq!(active["status"], "active");
    fixture
        .database
        .command()
        .args(["task", "activate", "FOO-0001"])
        .assert()
        .success()
        .stderr("")
        .stdout("Already active task: FOO-0001 :: ready later\n");
    assert_eq!(task_json(&fixture.database, &id).unwrap(), active);
    fixture
        .database
        .command()
        .args(["task", "reopen", "FOO-0001"])
        .assert()
        .failure();
    assert_eq!(task_json(&fixture.database, &id).unwrap(), active);
}

#[test]
fn closed_task_activation_requires_confirmation_before_removing_data() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command()
        .args(["task", "add", "foo", "finished / ship"])
        .assert()
        .success();
    fixture
        .database
        .command()
        .args(["task", "done", "FOO-0001", "-r", "verified"])
        .assert()
        .success();
    let id = task_id("FOO-0001").unwrap();
    let closed = task_json(&fixture.database, &id).unwrap();
    let output = fixture
        .database
        .command()
        .args(["task", "activate", "FOO-0001"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.stdout, b"");
    assert!(String::from_utf8(output.stderr).unwrap().contains("--yes"));
    assert_eq!(task_json(&fixture.database, &id).unwrap(), closed);
}

#[test]
fn task_content_formats_preserve_markdown_and_share_rich_output() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    fixture
        .database
        .command_args(&["add", "foo", "sample"])
        .success_stdout();
    let path = fixture
        .database
        .command_args(&["get", "FOO-0001", "--path"])
        .success_stdout();
    let path = path.trim();
    let source = "---\nid: FOO-0001\ntitle: Format Task\nstatus: active\ncreated_at: 2026-09-12T01:38:00-03:00\neffort: high\npriority: highest\n---\n\n## Goals\n\n- preserve **Markdown**\n  - nested item\n\n```rust\nlet x = 1;\n```\n";
    std::fs::write(path, source).unwrap();
    let get = fixture
        .database
        .command_args(&["get", "FOO-0001", "--long=rich"])
        .success_stdout();
    let list = fixture
        .database
        .command_args(&["list", "--project", "foo", "--long=rich"])
        .success_stdout();
    assert_eq!(get, list);
    assert!(
        get.starts_with(&format!("FOO-0001 :: Format Task\n\n  Path      {path}\n")),
        "{get}"
    );
    assert!(
        get.contains("  Priority  highest\n  Effort    high\n  Created   12/09/2026 01:38\n\n"),
        "{get}"
    );
    assert!(
        get.contains("## Goals\n\n- preserve **Markdown**\n  - nested item\n"),
        "{get}"
    );
    for arguments in [
        vec!["get", "FOO-0001"],
        vec!["get", "FOO-0001", "--long=md"],
        vec!["list", "--project", "foo", "--long"],
        vec!["list", "--project", "foo", "--long=md"],
    ] {
        let output = fixture
            .database
            .command_args(&arguments)
            .color()
            .success_stdout();
        assert_eq!(output, format!("{source}\n"), "{arguments:?}");
    }
    let single = fixture
        .database
        .command_args(&["get", "FOO-0001", "--long=json"])
        .success_json();
    let listed = fixture
        .database
        .command_args(&["list", "--project", "foo", "--long=json"])
        .success_json();
    assert_eq!(listed, serde_json::json!([single]));
    fixture
        .database
        .write_user_config("datetime_format = \"%Y-%m-%d %H:%M %:z\"\n")
        .unwrap();
    let output = fixture
        .database
        .command_args(&["get", "FOO-0001", "--long=rich"])
        .success_stdout();
    assert!(output.contains("2026-09-12 01:38 -03:00"), "{output}");
}

#[test]
fn task_content_format_parser_rejects_removed_and_conflicting_flags() {
    for arguments in [
        vec!["get", "FOO-0001", "--json"],
        vec!["list", "--json"],
        vec!["get", "FOO-0001", "--long=invalid"],
        vec!["list", "--long=invalid"],
        vec!["get", "FOO-0001", "--path", "--long=md"],
    ] {
        let output = command().args(&arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert_eq!(output.stdout, b"");
    }
}

#[test]
#[cfg(target_os = "linux")]
fn task_content_terminal_defaults_and_explicit_markdown_ignore_color_selection() {
    let project_name = format!("{}project", "readable-terminal-output-".repeat(4));
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), &project_name).unwrap();
    fixture
        .database
        .command_args(&["add", "foo", "sample"])
        .success_stdout();
    let path = fixture
        .database
        .command_args(&["get", "FOO-0001", "--path"])
        .success_stdout();
    fixture
        .database
        .write_user_config("[colors.task]\nactive = \"#010203\"\n")
        .unwrap();
    for arguments in [
        vec!["get", "FOO-0001"],
        vec!["get", "FOO-0001", "--long"],
        vec!["list", "--project", "foo", "--long"],
    ] {
        for color in [false, true] {
            let mut command = fixture.database.command_args(&arguments);
            if color {
                command.env_remove("NO_COLOR");
            }
            let mut session = expectrl::Session::spawn(command).unwrap();
            let (columns, _) = session.get_process().get_window_size().unwrap();
            session.set_expect_timeout(Some(std::time::Duration::from_secs(10)));
            let capture = session.expect(expectrl::Eof).unwrap();
            let output = String::from_utf8_lossy(capture.get(0).unwrap()).replace("\r\n", "\n");
            if color {
                let style = color_rgb(1, 2, 3);
                assert!(
                    output.starts_with(&format!("{style}FOO-0001{style:#} :: sample\n\n")),
                    "{output:?}"
                );
                let label = anstyle::Style::new()
                    .bold()
                    .fg_color(Some(anstyle::AnsiColor::Cyan.into()));
                assert!(
                    ["Path    ", "Priority", "Created "]
                        .into_iter()
                        .all(|field| output.contains(&format!("  {label}{field}{label:#}  "))),
                    "{output:?}"
                );
            } else {
                assert!(
                    output.starts_with("FOO-0001 :: sample\n\n  Path      "),
                    "{output:?}"
                );
                assert_plain(&output);
            }
            let plain = dialoguer::console::strip_ansi_codes(&output);
            let metadata = plain.split("\n\n").nth(1).unwrap();
            assert!(
                metadata
                    .lines()
                    .all(|line| line.len() <= usize::from(columns))
            );
            let (wrapped_path, _) = metadata.split_once("\n  Priority").unwrap();
            let (first, rest) = wrapped_path.split_once('\n').unwrap();
            let mut unwrapped_path = first.strip_prefix("  Path      ").unwrap().to_string();
            for line in rest.lines() {
                unwrapped_path.push_str(line.strip_prefix("            ").unwrap());
            }
            assert_eq!(unwrapped_path, path.trim());
            assert!(matches!(
                session.get_process().wait().unwrap(),
                expectrl::process::unix::WaitStatus::Exited(_, 0)
            ));
        }
    }
    for arguments in [
        vec!["get", "FOO-0001", "--long=md"],
        vec!["list", "--project", "foo", "--long=md"],
    ] {
        let mut command = fixture.database.command_args(&arguments);
        command.color();
        let mut session = expectrl::Session::spawn(command).unwrap();
        session.set_expect_timeout(Some(std::time::Duration::from_secs(10)));
        let capture = session.expect(expectrl::Eof).unwrap();
        let output = String::from_utf8_lossy(capture.get(0).unwrap()).replace("\r\n", "\n");
        assert!(output.starts_with("---\nid: FOO-0001\n"), "{output:?}");
        assert_plain(&output);
        assert!(matches!(
            session.get_process().wait().unwrap(),
            expectrl::process::unix::WaitStatus::Exited(_, 0)
        ));
    }
}

#[test]
fn task_content_lists_keep_machine_formats_clean_and_preserve_file_bytes() {
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo-bar").unwrap();
    let empty = fixture
        .database
        .command_args(&["list", "--project", "foo", "--long=json"])
        .success_json();
    assert_eq!(empty, serde_json::json!([]));
    for title in ["first", "second"] {
        fixture
            .database
            .command_args(&["add", "foo", title])
            .success_stdout();
    }
    let path = fixture
        .database
        .command_args(&["get", "FOO-0001", "--path"])
        .success_stdout();
    let source = "\u{feff}---\r\nid: FOO-0001\r\ntitle: first\r\nstatus: active\r\ncustom: retained\r\n---\r\n\r\n  authored body  \r\n";
    std::fs::write(path.trim(), source).unwrap();
    let markdown = fixture
        .database
        .command_args(&[
            "list",
            "--project",
            "foo",
            "--long=md",
            "--order=id:asc",
            "-n",
            "1",
        ])
        .color()
        .success_stdout();
    assert_eq!(markdown, format!("{source}\n"));
    let array = fixture
        .database
        .command_args(&["list", "--project", "foo", "--long=json", "-n", "1"])
        .color()
        .success_json();
    assert_eq!(array.as_array().unwrap().len(), 1);
    let rich = fixture
        .database
        .command_args(&["get", "FOO-0001", "--long=rich"])
        .success_stdout();
    assert!(rich.contains("\n\n  authored body  \r\n"), "{rich:?}");
    assert!(!rich.contains("1970"));
    fixture
        .database
        .write_user_config("datetime_format = \"%J\"\n")
        .unwrap();
    let rejected = fixture
        .database
        .command_args(&["get", "FOO-0001"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert_eq!(rejected.stdout, b"");
    let stderr = String::from_utf8(rejected.stderr).unwrap();
    assert!(
        stderr.contains("config.toml") && stderr.contains("datetime_format"),
        "{stderr}"
    );
}

#[test]
fn task_content_lists_render_all_pages_without_losing_file_bytes() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let project = directory.path().join("project");
    let tasks = directory.path().join("tasks");
    std::fs::create_dir_all(&project)?;
    std::fs::create_dir_all(&tasks)?;
    let mut sources = Vec::new();
    for number in 1..=257 {
        let metadata = if number == 1 {
            "created_at: 2026-09-12T01:38:00-03:00\r\neffort: high\r\npriority: highest\r\ntags: [rust, sqlite]\r\ncommits: a..b\r\nblocked_by: [\"[[FOO-0257]]\"]\r\n"
        } else {
            ""
        };
        let source = format!(
            "\u{feff}---\r\nid: FOO-{number:04}\r\ntitle: Task {number}\r\nstatus: active\r\n{metadata}---\r\n\r\n  authored body {number}  \r\n"
        );
        std::fs::write(tasks.join(format!("FOO-{number:04}.md")), &source)?;
        sources.push(source);
    }
    let fixture = ProjectFixture::new()?;
    fixture.add(
        &project_id("FOO")?,
        "foo",
        project.to_str().unwrap(),
        tasks.to_str().unwrap(),
    )?;
    let markdown = fixture.run(&["list", "--project", "foo", "--all", "--long=md"])?;
    crate::support::assert_success(&markdown, "list every Markdown page");
    assert_eq!(
        String::from_utf8(markdown.stdout)?,
        sources.into_iter().rev().collect::<Vec<_>>().join("\n") + "\n"
    );
    let json = fixture.run(&["list", "--project", "foo", "--all", "--long=json"])?;
    crate::support::assert_success(&json, "list every JSON page");
    let values: Vec<serde_json::Value> = serde_json::from_slice(&json.stdout)?;
    assert_eq!(values.len(), 257);
    for (index, value) in values.iter().enumerate() {
        let number = 257 - index;
        assert_eq!(value["id"], format!("FOO-{number:04}"));
        assert_eq!(value["body"], format!("authored body {number}"));
    }
    let final_task = values.last().unwrap();
    assert_eq!(final_task["created"], "2026-09-12");
    assert_eq!(final_task["effort"], "high");
    assert_eq!(final_task["priority"], "highest");
    assert_eq!(final_task["tags"], serde_json::json!(["rust", "sqlite"]));
    assert_eq!(final_task["commits"], "a..b");
    assert_eq!(final_task["blocked_by"], serde_json::json!(["FOO-0257"]));
    let rich = fixture.run(&["list", "--project", "foo", "--all", "--long=rich"])?;
    crate::support::assert_success(&rich, "list every rich page");
    let rich = String::from_utf8(rich.stdout)?;
    assert_eq!(
        rich.lines().filter(|line| line.starts_with("FOO-")).count(),
        257
    );
    assert!(
        rich.contains("  authored body 2  \r\n\nFOO-0001 :: Task 1\n\n"),
        "{rich}"
    );
    assert!(rich.contains("FOO-0257 (active)"), "{rich}");
    std::fs::write(
        tasks.join("FOO-0001.md"),
        "---\nid: FOO-0001\ntitle: Invalid final page\nstatus: active\nblocked_by: [not-a-wikilink]\n---\n\nbody\n",
    )?;
    let rejected = fixture.run(&["list", "--project", "foo", "--all", "--long=json"])?;
    assert!(!rejected.status.success());
    assert_eq!(rejected.stdout, b"");
    assert!(String::from_utf8(rejected.stderr)?.contains("Malformed blocked_by metadata"));
    Ok(())
}
