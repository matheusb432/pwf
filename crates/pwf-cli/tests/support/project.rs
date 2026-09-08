use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use pwf_models::project::ProjectId;
use serde_json::{Value, json};
use tempfile::TempDir;

use super::{DatabaseFixture, command};

pub struct ProjectFixture {
    database: DatabaseFixture,
    _directory: TempDir,
}

impl ProjectFixture {
    pub fn new() -> anyhow::Result<Self> {
        let directory = tempfile::tempdir()?;
        let database = DatabaseFixture::new(directory.path().join("projects.sqlite3"))?;
        Ok(Self {
            database,
            _directory: directory,
        })
    }

    pub fn run(&self, arguments: &[&str]) -> std::io::Result<Output> {
        self.database.command().args(arguments).output()
    }

    pub fn run_with_home(&self, arguments: &[&str], home: &Path) -> std::io::Result<Output> {
        self.database
            .command_with_home(home)
            .args(arguments)
            .output()
    }

    pub fn add(
        &self,
        project_id: &ProjectId,
        title: &str,
        project_path: &str,
        tasks_path: &str,
    ) -> anyhow::Result<Value> {
        success_json(self.run(&[
            "project",
            "add",
            "--kind",
            "directory",
            &add_payload(project_id, title, project_path, tasks_path),
        ])?)
    }

    pub fn add_with_home(
        &self,
        project_id: &ProjectId,
        title: &str,
        project_path: &str,
        tasks_path: &str,
        home: &Path,
    ) -> anyhow::Result<Value> {
        success_json(self.run_with_home(
            &[
                "project",
                "add",
                "--kind",
                "directory",
                &add_payload(project_id, title, project_path, tasks_path),
            ],
            home,
        )?)
    }
}

pub fn add_payload(
    project_id: &ProjectId,
    title: &str,
    project_path: &str,
    tasks_path: &str,
) -> String {
    json!({
        "id": project_id.as_ref(),
        "title": title,
        "source": {"value": project_path},
        "tasks": {"kind": "directory", "path": tasks_path},
    })
    .to_string()
}

pub fn success_json(output: Output) -> anyhow::Result<Value> {
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        output.status.success(),
        "expected success\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stderr.is_empty(), "successful command stderr: {stderr}");
    Ok(serde_json::from_str(&stdout)?)
}

pub fn assert_project(
    project: &Value,
    project_id: &ProjectId,
    title: &str,
    project_path: &str,
    tasks_path: &str,
    is_paused: bool,
) {
    assert_eq!(project["id"], project_id.as_ref());
    assert_eq!(project["title"], title);
    assert_eq!(
        project["source"],
        json!({"kind": "directory", "value": project_path})
    );
    assert_eq!(
        project["tasks"],
        json!({"kind": "directory", "path": tasks_path})
    );
    assert!(
        project["created_at"]
            .as_str()
            .is_some_and(|created_at| created_at.ends_with('Z')),
        "created_at should be a UTC timestamp: {project}"
    );
    assert_eq!(project["is_paused"], is_paused);
    assert_eq!(
        project.get("obsidian_vault"),
        Some(&serde_json::Value::Null)
    );
    assert_eq!(project.as_object().map(serde_json::Map::len), Some(7));
}

pub fn assert_failure(output: Output, identifying_fragments: &[&str]) -> anyhow::Result<()> {
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    assert!(!output.status.success(), "command should fail: {stdout}");
    assert!(stdout.is_empty(), "failed command stdout: {stdout}");
    assert!(
        !stderr.is_empty(),
        "failed command should explain the failure"
    );
    for fragment in identifying_fragments {
        assert!(
            stderr.contains(fragment),
            "stderr should identify {fragment:?}: {stderr}"
        );
    }
    Ok(())
}

pub fn run_server_with_database(database_path: &Path) -> std::io::Result<Output> {
    let root = database_path.parent().unwrap_or(database_path);
    Command::new(super::binary_path("pwf-server"))
        .env("PWF_DATABASE_PATH", database_path)
        .env("PWF_RUNTIME_DIR", root.join("server-data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("HOME", root.join("home"))
        .env("RUST_LOG", "warn")
        .output()
}

pub fn write_project_rename_fixture(tasks_path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(tasks_path)?;
    fs::write(
        tasks_path.join("sample-app.md"),
        "---\nid: old\ntitle: sample-app\n---\n\n- [ ] [[OLD-0079]]\n",
    )?;
    fs::write(
        tasks_path.join("OLD-0079.md"),
        "---\nid: OLD-0079\nstatus: active\ntitle: keep body\nproject: sample-app\ncreated: 2026-07-01\n---\n\nTask body remains intact.\n",
    )?;
    Ok(())
}
