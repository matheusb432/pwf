#![cfg(unix)]

use std::fs;

use assert_cmd::prelude::OutputAssertExt as _;

use crate::support::{SessionFixture, task_id, task_json};

#[test]
fn session_ignores_invalid_timestamps_on_unselected_tasks() {
    let fixture = SessionFixture::new().unwrap();
    let notes = fixture.directory().join("notes/foo");
    let source = fs::read_to_string(notes.join("FOO-0001.md")).unwrap();
    let broken = source
        .replace("FOO-0001", "FOO-0002")
        .replace("created_at: ", "created_at: invalid-");
    fs::write(notes.join("FOO-0002.md"), broken).unwrap();

    fixture
        .database
        .command()
        .args(["session", "foo1", "--dry"])
        .env("PATH", &fixture.child_path)
        .assert()
        .success();
    fixture
        .database
        .command()
        .args(["session", "foo2", "--dry"])
        .env("PATH", &fixture.child_path)
        .assert()
        .failure();
}

#[test]
fn codex_dry_run_forwards_max_reasoning_effort() {
    let fixture = SessionFixture::new().unwrap();
    let task_before = task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap();
    let app_server_log_path = fixture.directory().join("codex-app-server.jsonl");
    let resume_log_path = fixture.directory().join("codex-resume.log");

    let assertion = fixture
        .database
        .command()
        .args([
            "session",
            "--id",
            "foo1",
            "--agent",
            "codex",
            "--dry",
            "--effort",
            "max",
            "--push-prompt",
            "dry-run context",
        ])
        .env("PATH", &fixture.child_path)
        .env("CODEX_STUB_APP_SERVER_LOG", &app_server_log_path)
        .env("CODEX_STUB_RESUME_LOG", &resume_log_path)
        .assert()
        .success();

    let stdout = String::from_utf8(assertion.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("effort: max"), "{stdout}");
    assert!(
        stdout.contains("-c 'model_reasoning_effort=\"max\"'"),
        "{stdout}"
    );
    assert!(stdout.contains("<pwf_session_context>"), "{stdout}");
    assert!(stdout.contains("dry-run context"), "{stdout}");
    assert!(stdout.contains("</pwf_session_context>"), "{stdout}");
    assert!(stdout.contains("<pwf_task>"), "{stdout}");
    assert_eq!(
        task_json(&fixture.database, &task_id("FOO-0001").unwrap()).unwrap(),
        task_before
    );
    assert!(!app_server_log_path.exists());
    assert!(!resume_log_path.exists());
}

#[test]
fn default_dispatch_forwards_the_explicit_model_to_the_concrete_claude_process() {
    let fixture = SessionFixture::new().unwrap();
    fixture.install_claude().unwrap();
    let claude_log_path = fixture.directory().join("claude.log");

    fixture
        .database
        .command()
        .args([
            "session",
            "--id",
            "FOO-0001",
            "--agent",
            "claude",
            "--yes",
            "--model",
            "manual-model",
        ])
        .env("PATH", &fixture.child_path)
        .env("CLAUDE_STUB_EXIT_CODE", "23")
        .env("CLAUDE_STUB_LOG", &claude_log_path)
        .assert()
        .code(23);

    let project_path = fixture.directory().join("project");
    let log = fs::read(&claude_log_path).unwrap();
    let entries = log
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| String::from_utf8(entry.to_vec()).unwrap())
        .collect::<Vec<_>>();
    let working_directory = std::path::Path::new(entries[0].strip_prefix("cwd=").unwrap());
    assert_eq!(
        working_directory.canonicalize().unwrap(),
        project_path.canonicalize().unwrap()
    );
    assert_eq!(
        entries[1..8],
        [
            "arg=--name",
            "arg=foo1 :: do the thing",
            "arg=--model",
            "arg=manual-model",
            "arg=--effort",
            "arg=high",
            "arg=--",
        ]
    );
    assert!(entries[8].starts_with("arg=<pwf_task>\n"));
    assert!(entries[8].contains("do the thing"));
    assert!(entries[8].ends_with("\n</pwf_task>"));
}

#[test]
fn personal_separator_names_both_agent_sessions() -> anyhow::Result<()> {
    let fixture = SessionFixture::new()?;
    fixture.install_claude()?;
    fixture
        .database
        .write_title_config("[task]\nseparator = \" \"\n")?;
    let claude_log = fixture.directory().join("claude-title.log");
    fixture
        .database
        .command()
        .args(["session", "foo1", "--agent", "claude", "--yes"])
        .env("PATH", &fixture.child_path)
        .env("CLAUDE_STUB_LOG", &claude_log)
        .assert()
        .success();
    let log = fs::read_to_string(claude_log)?;
    assert!(log.contains("arg=foo1 do the thing\0"), "{log}");

    let codex_log = fixture.directory().join("codex-title.jsonl");
    fixture
        .database
        .command()
        .args(["session", "foo1", "--agent", "codex", "--yes"])
        .env("PATH", &fixture.child_path)
        .env("CODEX_STUB_APP_SERVER_LOG", &codex_log)
        .env(
            "CODEX_STUB_RESUME_LOG",
            fixture.directory().join("codex-title-resume.log"),
        )
        .assert()
        .success();
    let requests = fs::read_to_string(codex_log)?
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    let name = requests
        .iter()
        .find(|request| request["method"] == "thread/name/set")
        .ok_or_else(|| anyhow::anyhow!("no Codex thread naming request"))?;
    assert_eq!(name["params"]["name"], "foo1 do the thing");
    Ok(())
}
