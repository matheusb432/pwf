use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context as _, bail, ensure};
use serde_json::{Value, json};
use tempfile::TempDir;

const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(5);

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn release_binary(name: &str) -> PathBuf {
    repository_root()
        .join("target")
        .join("release")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// Isolates pwf state, the server endpoint, and Neovim state under one temporary directory.
pub struct Fixture {
    directory: TempDir,
    server: Option<Child>,
}

pub struct Project {
    pub id: &'static str,
    pub source: PathBuf,
}

impl Fixture {
    pub fn with_server() -> anyhow::Result<Self> {
        let mut fixture = Self::without_server()?;
        let log = fs::File::create(fixture.path("pwf-server.stderr.log"))?;
        let mut command = Command::new(release_binary("pwf-server"));
        fixture.configure(&mut command);
        fixture.server = Some(
            command
                .env("RUST_LOG", "warn")
                .stdout(Stdio::null())
                .stderr(Stdio::from(log))
                .spawn()?,
        );
        fixture.wait_until_ready()?;
        Ok(fixture)
    }

    pub fn without_server() -> anyhow::Result<Self> {
        let fixture = Self {
            directory: tempfile::tempdir()?,
            server: None,
        };
        // The server creates `runtime` itself with private permissions.
        for directory in ["home/.config", "xdg-state", "xdg-data"] {
            fs::create_dir_all(fixture.path(directory))?;
        }
        Ok(fixture)
    }

    pub fn directory(&self) -> &Path {
        self.directory.path()
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.directory.path().join(relative)
    }

    fn configure(&self, command: &mut Command) {
        command
            .env("PWF_DATABASE_PATH", self.path("projects.sqlite3"))
            .env("PWF_RUNTIME_DIR", self.path("runtime"))
            .env("HOME", self.path("home"))
            .env("USERPROFILE", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("home/.config"))
            .env("XDG_STATE_HOME", self.path("xdg-state"))
            .env("XDG_DATA_HOME", self.path("xdg-data"))
            .env("NO_COLOR", "1");
    }

    fn wait_until_ready(&mut self) -> anyhow::Result<()> {
        let deadline = Instant::now() + SERVER_READY_TIMEOUT;
        loop {
            let server = self.server.as_mut().context("no server was started")?;
            if let Some(status) = server.try_wait()? {
                bail!("pwf-server exited with {status}: {}", self.server_log());
            }
            let mut probe = Command::new(release_binary("pwf"));
            self.configure(&mut probe);
            let ready = probe
                .args(["project", "ls"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if ready {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("pwf-server did not become ready: {}", self.server_log());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn server_log(&self) -> String {
        fs::read_to_string(self.path("pwf-server.stderr.log")).unwrap_or_default()
    }

    pub fn pwf(&self, arguments: &[&str]) -> anyhow::Result<String> {
        let mut command = Command::new(release_binary("pwf"));
        self.configure(&mut command);
        let output = command.args(arguments).output()?;
        ensure!(
            output.status.success(),
            "pwf {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?)
    }

    /// Registers a directory project with an Obsidian task folder and a source directory.
    pub fn add_project(&self, id: &'static str, title: &str) -> anyhow::Result<Project> {
        let vault = self.path("vault");
        let tasks = vault.join(title);
        let source = self.path("src").join(title);
        fs::create_dir_all(vault.join(".obsidian"))?;
        fs::create_dir_all(&tasks)?;
        fs::create_dir_all(&source)?;
        fs::write(
            tasks.join(format!("{title}.md")),
            format!(
                "---\nid: {}\ntitle: {title}\n---\n",
                id.to_ascii_lowercase()
            ),
        )?;
        let payload = json!({
            "id": id,
            "title": title,
            "source": {"value": source},
            "tasks": {"kind": "directory", "path": tasks},
        });
        self.pwf(&[
            "project",
            "add",
            "--kind",
            "directory",
            &payload.to_string(),
        ])?;
        Ok(Project { id, source })
    }

    /// Adds an active task and returns its ID.
    pub fn add_task(&self, project: &Project, title: &str) -> anyhow::Result<String> {
        let output = self.pwf(&["task", "add", project.id, title])?;
        output
            .split_whitespace()
            .nth(2)
            .map(str::to_string)
            .with_context(|| format!("unexpected task add output: {output}"))
    }

    pub fn task_path(&self, id: &str) -> anyhow::Result<PathBuf> {
        Ok(PathBuf::from(
            self.pwf(&["task", "get", id, "--path"])?.trim(),
        ))
    }

    /// Runs `script` as a Lua function body in headless Neovim with the plugin on the runtime
    /// path, and decodes the JSON of its return value. `await(start)` passes a node-style callback
    /// to `start` and returns `{ err, value }` once it is called.
    pub fn run_lua(&self, cwd: &Path, script: &str) -> anyhow::Result<Value> {
        let script_path = self.path("script.lua");
        fs::write(
            &script_path,
            format!(
                r#"vim.opt.runtimepath:prepend([==[{root}]==])
require("pwf").setup({{ cmd = {{ [==[{child}]==] }} }})
function await(start)
  local finished, outcome = false, nil
  start(function(err, value)
    finished, outcome = true, {{ err = err, value = value }}
  end)
  assert(vim.wait(20000, function() return finished end, 10), "timed out waiting for pwf")
  return outcome
end
local result = (function()
{script}
end)()
io.stdout:write(vim.json.encode(result))
"#,
                root = repository_root().display(),
                child = release_binary("pwf-nvim").display(),
            ),
        )?;
        let mut command = Command::new("nvim");
        self.configure(&mut command);
        let output = command
            .args(["--clean", "-n", "-l"])
            .arg(&script_path)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .output()
            .context("run nvim; `mise install` provides it")?;
        ensure!(
            output.status.success(),
            "nvim failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).with_context(|| {
            format!(
                "nvim printed invalid JSON: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(server) = self.server.as_mut() {
            let _ = server.kill();
            let _ = server.wait();
        }
    }
}
