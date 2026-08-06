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
use pwf_models::{project::ProjectId, task::TaskId};
use serde_json::{Value, json};
#[cfg(unix)]
pub use session::SessionFixture;
use tempfile::TempDir;

fn binary_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("E2E crate is under the workspace root")
        .join("target")
        .join("release")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

pub fn command() -> Command {
    Command::new(binary_path("pwf"))
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
        let fixture = Self { path, home };
        let output = Command::new(binary_path("pwf-migrator"))
            .env("PWF_DATABASE_PATH", &fixture.path)
            .output()
            .expect("run pwf-migrator process");
        assert_success(&output, "migrate test database");
        fixture
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
        project_id: &ProjectId,
        title: &str,
        project_path: &Path,
        tasks_path: &Path,
    ) {
        let payload = json!({
            "id": project_id.as_ref(),
            "title": title,
            "source": {"value": project_path},
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
    pub fn new(project_id: &ProjectId, title: &str) -> Self {
        let directory = TempDir::new().unwrap();
        let tasks_path = directory.path().join("notes").join(title);
        let project_path = directory.path().join("project");
        fs::create_dir_all(&tasks_path).unwrap();
        fs::create_dir_all(&project_path).unwrap();
        fs::write(
            tasks_path.join(format!("{title}.md")),
            format!(
                "---\nid: {}\ntitle: {title}\n---\n",
                project_id.as_ref().to_ascii_lowercase()
            ),
        )
        .unwrap();
        let database = DatabaseFixture::new(directory.path().join("projects.sqlite3"));
        database.add_directory_project(project_id, title, &project_path, &tasks_path);
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

pub fn project_id(raw: &str) -> ProjectId {
    ProjectId::try_new(raw).expect("fixture project ID is valid")
}

pub fn task_id(raw: &str) -> TaskId {
    TaskId::try_new(raw).expect("fixture task ID is valid")
}

pub fn task_json(database: &DatabaseFixture, task_id: &TaskId) -> Value {
    let output = database
        .command()
        .args(["task", "show", task_id.as_ref(), "--json"])
        .output()
        .unwrap();
    assert_success(&output, &format!("show {task_id}"));
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
