mod project;
#[cfg(unix)]
mod session;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context as _, bail};
use assert_cmd::prelude::OutputAssertExt as _;
pub use project::{
    ProjectFixture, add_payload, assert_failure, assert_project, run_server_with_database,
    success_json, write_project_rename_fixture,
};
use pwf_models::{project::ProjectId, task::TaskId};
use serde_json::{Value, json};
#[cfg(unix)]
pub use session::SessionFixture;
use tempfile::TempDir;

fn binary_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target")
        .join("release")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

pub fn command() -> Command {
    Command::new(binary_path("pwf"))
}

pub struct DatabaseFixture {
    server: ServerProcess,
    path: PathBuf,
    home: PathBuf,
}

impl DatabaseFixture {
    pub fn new(path: PathBuf) -> anyhow::Result<Self> {
        let home = path
            .parent()
            .context("database fixture path has no parent")?
            .join("home");
        fs::create_dir_all(&home)?;
        let output = Command::new(binary_path("pwf-migrator"))
            .env("PWF_DATABASE_PATH", &path)
            .output()?;
        assert_success(&output, "migrate test database");
        let server = ServerProcess::start(&path, &home)?;
        Ok(Self { server, path, home })
    }

    pub fn command(&self) -> Command {
        self.command_with_home(&self.home)
    }

    pub fn command_with_home(&self, home: &Path) -> Command {
        let mut command = command();
        configure_command(&mut command, &self.path, home, &self.server.data_root);
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

struct ServerProcess {
    child: Child,
    data_root: PathBuf,
    log_path: PathBuf,
}

impl ServerProcess {
    fn start(database_path: &Path, home: &Path) -> anyhow::Result<Self> {
        let root = database_path
            .parent()
            .context("database fixture path has no parent")?;
        let data_root = root.join("pwf-server-data");
        let state_root = root.join("xdg-state");
        let local_data_root = root.join("xdg-data");
        let log_path = root.join("pwf-server.stderr.log");
        fs::create_dir_all(&state_root)?;
        fs::create_dir_all(&local_data_root)?;
        let stderr = fs::File::create(&log_path)?;
        let mut command = Command::new(binary_path("pwf-server"));
        configure_command(&mut command, database_path, home, &data_root);
        let child = command
            .env("XDG_STATE_HOME", &state_root)
            .env("XDG_DATA_HOME", &local_data_root)
            .env("RUST_LOG", "warn")
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr))
            .spawn()?;
        let mut server = Self {
            child,
            data_root,
            log_path,
        };
        wait_until_ready(&mut server, database_path, home)?;
        Ok(server)
    }
}

fn wait_until_ready(
    server: &mut ServerProcess,
    database_path: &Path,
    home: &Path,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = server.child.try_wait()? {
            bail!(
                "pwf-server exited with {status}: {}",
                fs::read_to_string(&server.log_path).unwrap_or_default()
            );
        }
        let mut probe = command();
        configure_command(&mut probe, database_path, home, &server.data_root);
        if probe
            .args(["project", "ls"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "pwf-server did not become ready: {}",
                fs::read_to_string(&server.log_path).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn configure_command(command: &mut Command, database_path: &Path, home: &Path, data_root: &Path) {
    command
        .env("PWF_DATABASE_PATH", database_path)
        .env("PWF_DATA_DIR", data_root)
        .env("HOME", home)
        .env("USERPROFILE", home);
}

pub struct ManagedProject {
    pub database: DatabaseFixture,
    _directory: TempDir,
}

impl ManagedProject {
    pub fn new(project_id: &ProjectId, title: &str) -> anyhow::Result<Self> {
        let directory = TempDir::new()?;
        let tasks_path = directory.path().join("notes").join(title);
        let project_path = directory.path().join("project");
        fs::create_dir_all(&tasks_path)?;
        fs::create_dir_all(&project_path)?;
        fs::write(
            tasks_path.join(format!("{title}.md")),
            format!(
                "---\nid: {}\ntitle: {title}\n---\n",
                project_id.as_ref().to_ascii_lowercase()
            ),
        )?;
        let database = DatabaseFixture::new(directory.path().join("projects.sqlite3"))?;
        database.add_directory_project(project_id, title, &project_path, &tasks_path);
        Ok(Self {
            database,
            _directory: directory,
        })
    }
}

pub fn project_id(raw: &str) -> anyhow::Result<ProjectId> {
    Ok(ProjectId::try_new(raw)?)
}

pub fn task_id(raw: &str) -> anyhow::Result<TaskId> {
    Ok(TaskId::try_new(raw)?)
}

pub fn task_json(database: &DatabaseFixture, task_id: &TaskId) -> anyhow::Result<Value> {
    let output = database
        .command()
        .args(["task", "get", task_id.as_ref(), "--json"])
        .output()?;
    assert_success(&output, &format!("get {task_id}"));
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub fn assert_success(output: &Output, operation: &str) {
    assert!(
        output.status.success(),
        "{operation} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
