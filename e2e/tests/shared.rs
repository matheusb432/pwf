mod project;
#[cfg(unix)]
mod session;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use assert_cmd::prelude::OutputAssertExt as _;
pub use project::{
    ProjectFixture, add_payload, assert_failure, assert_project, run_with_database, success_json,
};
use serde_json::{Value, json};
#[cfg(unix)]
pub use session::SessionFixture;
use tempfile::TempDir;

fn binary_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("E2E crate is under the workspace root")
        .join("target")
        .join("release")
        .join(format!("pwf{}", std::env::consts::EXE_SUFFIX))
}

pub fn command() -> Command {
    Command::new(binary_path())
}

pub struct DatabaseFixture {
    path: PathBuf,
    home: PathBuf,
}

impl DatabaseFixture {
    pub fn new(path: PathBuf) -> Self {
        let home = path
            .parent()
            .expect("database fixture path has a parent")
            .join("home");
        fs::create_dir_all(&home).expect("create isolated home");
        Self { path, home }
    }

    pub fn command(&self) -> Command {
        self.command_with_home(&self.home)
    }

    pub fn command_with_home(&self, home: &Path) -> Command {
        let mut command = command();
        command
            .env("PWF_DATABASE_PATH", &self.path)
            .env("HOME", home)
            .env("USERPROFILE", home);
        command
    }

    pub fn add_directory_project(
        &self,
        id: &str,
        title: &str,
        repository: &Path,
        tasks_path: &Path,
    ) {
        let payload = json!({
            "id": id,
            "title": title,
            "source": {"value": repository},
            "tasks": {"kind": "directory", "path": tasks_path},
        })
        .to_string();
        self.command()
            .args(["project", "add", "--kind", "directory", &payload])
            .assert()
            .success();
    }
}

pub struct ManagedProject {
    _directory: TempDir,
    pub database: DatabaseFixture,
    tasks_path: PathBuf,
}

impl ManagedProject {
    pub fn new(id: &str, title: &str) -> Self {
        let directory = TempDir::new().unwrap();
        let tasks_path = directory.path().join("notes").join(title);
        let repository = directory.path().join("repo");
        fs::create_dir_all(&tasks_path).unwrap();
        fs::create_dir_all(&repository).unwrap();
        fs::write(
            tasks_path.join(format!("{title}.md")),
            format!(
                "---\nid: {}\ntitle: {title}\n---\n",
                id.to_ascii_lowercase()
            ),
        )
        .unwrap();
        let database = DatabaseFixture::new(directory.path().join("projects.sqlite3"));
        database.add_directory_project(id, title, &repository, &tasks_path);
        Self {
            _directory: directory,
            database,
            tasks_path,
        }
    }

    pub fn note_markdown(&self, id: &str) -> String {
        fs::read_to_string(self.tasks_path.join(format!("{id}.md"))).unwrap()
    }
}

pub fn task_json(database: &DatabaseFixture, id: &str) -> Value {
    let output = database
        .command()
        .args(["show", id, "--json"])
        .output()
        .unwrap();
    assert_success(&output, &format!("show {id}"));
    serde_json::from_slice(&output.stdout).expect("show stdout is JSON")
}

pub fn assert_success(output: &Output, operation: &str) {
    assert!(
        output.status.success(),
        "{operation} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
