use std::{
    path::{Path, PathBuf},
    process::Command,
};

use assert_cmd::prelude::OutputAssertExt as _;
use serde_json::json;

pub struct DatabaseFixture {
    path: PathBuf,
}

impl DatabaseFixture {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pwf"));
        command.env("PWF_DATABASE_PATH", &self.path);
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
