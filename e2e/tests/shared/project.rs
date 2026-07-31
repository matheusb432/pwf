use std::{path::Path, process::Output};

use serde_json::{Value, json};
use tempfile::TempDir;

use super::{DatabaseFixture, command};

pub struct ProjectFixture {
    _directory: TempDir,
    database: DatabaseFixture,
}

impl ProjectFixture {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().expect("create project fixture directory");
        let database = DatabaseFixture::new(directory.path().join("projects.sqlite3"));
        Self {
            _directory: directory,
            database,
        }
    }

    pub fn run(&self, arguments: &[&str]) -> Output {
        self.database
            .command()
            .args(arguments)
            .output()
            .expect("run pwf process")
    }

    pub fn run_with_home(&self, arguments: &[&str], home: &Path) -> Output {
        self.database
            .command_with_home(home)
            .args(arguments)
            .output()
            .expect("run pwf process")
    }

    pub fn add(&self, id: &str, title: &str, source: &str, tasks: &str) -> Value {
        success_json(self.run(&[
            "project",
            "add",
            "--kind",
            "directory",
            &add_payload(id, title, source, tasks),
        ]))
    }

    pub fn add_with_home(
        &self,
        id: &str,
        title: &str,
        source: &str,
        tasks: &str,
        home: &Path,
    ) -> Value {
        success_json(self.run_with_home(
            &[
                "project",
                "add",
                "--kind",
                "directory",
                &add_payload(id, title, source, tasks),
            ],
            home,
        ))
    }
}

pub fn add_payload(id: &str, title: &str, source: &str, tasks: &str) -> String {
    json!({
        "id": id,
        "title": title,
        "source": {"value": source},
        "tasks": {"kind": "directory", "path": tasks},
    })
    .to_string()
}

pub fn success_json(output: Output) -> Value {
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        output.status.success(),
        "expected success\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stderr.is_empty(), "successful command stderr: {stderr}");
    serde_json::from_str(&stdout).expect("stdout is JSON")
}

pub fn assert_project(
    project: &Value,
    id: &str,
    title: &str,
    source: &str,
    tasks: &str,
    is_paused: bool,
) {
    assert_eq!(project["id"], id);
    assert_eq!(project["title"], title);
    assert_eq!(
        project["source"],
        json!({"kind": "directory", "value": source})
    );
    assert_eq!(
        project["tasks"],
        json!({"kind": "directory", "path": tasks})
    );
    assert!(
        project["created_at"]
            .as_str()
            .is_some_and(|created_at| created_at.ends_with('Z')),
        "created_at should be a UTC timestamp: {project}"
    );
    assert_eq!(project["is_paused"], is_paused);
    assert_eq!(project.as_object().map(serde_json::Map::len), Some(6));
}

pub fn assert_failure(output: Output, identifying_fragments: &[&str]) {
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
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
}

pub fn run_with_database(database_path: &Path, arguments: &[&str]) -> Output {
    command()
        .args(arguments)
        .env("PWF_DATABASE_PATH", database_path)
        .output()
        .expect("run pwf process")
}
