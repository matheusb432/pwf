use std::{
    fs,
    path::{Path, PathBuf},
};

use assert_cmd::prelude::OutputAssertExt as _;
use tempfile::TempDir;

use super::{DatabaseFixture, project_id};

pub struct SessionFixture {
    pub database: DatabaseFixture,
    directory: TempDir,
    pub child_path: String,
    pub tmux_log_path: PathBuf,
}

impl SessionFixture {
    pub fn new() -> anyhow::Result<Self> {
        let directory = TempDir::new()?;
        let notes = directory.path().join("notes/foo");
        let project_path = directory.path().join("project");
        fs::create_dir_all(&notes)?;
        fs::create_dir_all(&project_path)?;
        fs::write(notes.join("foo.md"), "---\nid: foo\ntitle: foo\n---\n")?;
        let database = DatabaseFixture::new(directory.path().join("projects.sqlite3"))?;
        database.add_directory_project(&project_id("FOO")?, "foo", &project_path, &notes);
        database
            .command()
            .args([
                "task",
                "add",
                "foo",
                "--title",
                "do the thing",
                "--goal",
                "do the thing",
            ])
            .assert()
            .success();

        let binary_directory = directory.path().join("bin");
        fs::create_dir_all(&binary_directory)?;
        for name in ["tmux", "codex"] {
            install_fixture(&binary_directory, name)?;
        }
        let child_path = format!(
            "{}:{}",
            binary_directory.to_string_lossy(),
            std::env::var("PATH").unwrap_or_default()
        );
        let tmux_log_path = directory.path().join("tmux.log");

        Ok(Self {
            database,
            directory,
            child_path,
            tmux_log_path,
        })
    }

    pub fn directory(&self) -> &Path {
        self.directory.path()
    }

    pub fn install_claude(&self) -> std::io::Result<PathBuf> {
        let path = self.directory.path().join("bin/claude");
        install_fixture_at(&path, "claude")?;
        Ok(path)
    }
}

fn install_fixture(binary_directory: &Path, name: &str) -> std::io::Result<()> {
    install_fixture_at(&binary_directory.join(name), name)
}

fn install_fixture_at(destination: &Path, name: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}-stub.sh"));
    fs::copy(source, destination)?;
    fs::set_permissions(destination, fs::Permissions::from_mode(0o755))?;
    Ok(())
}
