use std::{
    fs,
    path::{Path, PathBuf},
};

use assert_cmd::prelude::OutputAssertExt as _;
use tempfile::TempDir;

use super::DatabaseFixture;

pub struct SessionFixture {
    directory: TempDir,
    pub database: DatabaseFixture,
    pub child_path: String,
    pub tmux_log_path: PathBuf,
}

impl SessionFixture {
    pub fn new() -> Self {
        let directory = TempDir::new().unwrap();
        let notes = directory.path().join("notes/pwf");
        let repository = directory.path().join("repo");
        fs::create_dir_all(&notes).unwrap();
        fs::create_dir_all(&repository).unwrap();
        fs::write(notes.join("pwf.md"), "---\nid: pwf\ntitle: pwf\n---\n").unwrap();
        let database = DatabaseFixture::new(directory.path().join("projects.sqlite3"));
        database.add_directory_project("PWF", "pwf", &repository, &notes);
        database
            .command()
            .args([
                "add",
                "pwf",
                "do the thing",
                "--title",
                "do the thing",
                "--date",
                "2026-06-20",
            ])
            .assert()
            .success();

        let binary_directory = directory.path().join("bin");
        fs::create_dir_all(&binary_directory).unwrap();
        for name in ["tmux", "codex"] {
            install_fixture(&binary_directory, name);
        }
        let child_path = format!(
            "{}:{}",
            binary_directory.to_string_lossy(),
            std::env::var("PATH").unwrap_or_default()
        );
        let tmux_log_path = directory.path().join("tmux.log");

        Self {
            directory,
            database,
            child_path,
            tmux_log_path,
        }
    }

    pub fn directory(&self) -> &Path {
        self.directory.path()
    }

    pub fn install_claude(&self) -> PathBuf {
        let path = self.directory.path().join("bin/claude");
        install_fixture_at(&path, "claude");
        path
    }
}

fn install_fixture(binary_directory: &Path, name: &str) {
    install_fixture_at(&binary_directory.join(name), name);
}

fn install_fixture_at(destination: &Path, name: &str) {
    use std::os::unix::fs::PermissionsExt;

    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(format!("{name}-stub.sh"));
    fs::copy(source, destination).unwrap();
    fs::set_permissions(destination, fs::Permissions::from_mode(0o755)).unwrap();
}
