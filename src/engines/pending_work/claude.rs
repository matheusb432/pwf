// Claude CLI probe, command construction, verify output, and the direct
// claude launch.

use super::errors;
use super::launch::new_launch_prompt;
use super::model::Item;
use super::query::find_pending_item;
use crate::config::Config;

// ── ClaudeProbe trait + impls ─────────────────────────────────────────────────

pub trait ClaudeProbe {
    fn available(&self) -> bool;
    fn path(&self) -> Option<&str>;
    fn version(&self) -> Option<&str>;
    fn interactive(&self) -> bool;
}

/// Real probe: resolves `claude` on PATH, queries version, checks stdin TTY.
pub struct RealProbe {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub interactive: bool,
}

impl RealProbe {
    pub fn resolve() -> Self {
        // Find `claude` on PATH.
        let cmd_result = which_claude();
        let (available, path, version) = match cmd_result {
            None => (false, None, None),
            Some(p) => {
                let ver = run_claude_version(&p);
                (true, Some(p), ver)
            }
        };
        let interactive = is_interactive_console();
        RealProbe {
            available,
            path,
            version,
            interactive,
        }
    }
}

fn which_claude() -> Option<String> {
    // Windows-only: resolve `claude` on PATH via `where` (this engine ships as pw.exe).
    let output = std::process::Command::new("where")
        .arg("claude")
        .output()
        .ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout);
        let first = s.lines().next()?;
        return Some(first.trim().to_string());
    }
    None
}

fn run_claude_version(path: &str) -> Option<String> {
    let output = std::process::Command::new(path)
        .arg("--version")
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout);
    s.lines().next().map(|l| l.trim().to_string())
}

fn is_interactive_console() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

impl ClaudeProbe for RealProbe {
    fn available(&self) -> bool {
        self.available
    }
    fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }
    fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
    fn interactive(&self) -> bool {
        self.interactive
    }
}

/// Fake probe for tests.
pub struct FakeProbe {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub interactive: bool,
}

impl ClaudeProbe for FakeProbe {
    fn available(&self) -> bool {
        self.available
    }
    fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }
    fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
    fn interactive(&self) -> bool {
        self.interactive
    }
}

fn new_claude_launch_command(title: &str, prompt: &str, claude_path: Option<&str>) -> Vec<String> {
    let exe = claude_path.unwrap_or("claude").to_string();
    vec![
        exe,
        "--name".to_string(),
        title.to_string(),
        prompt.to_string(),
    ]
}

fn get_claude_command_display(title: &str, prompt: &str) -> String {
    let lines: Vec<&str> = prompt.lines().collect();
    let shown = if lines.len() > 1 {
        format!("{}…", lines[0])
    } else {
        lines.first().copied().unwrap_or("").to_string()
    };
    format!("claude --name \"{title}\" \"{shown}\"")
}

/// Build the verify JSON output given an optional item and a probe.
pub fn verify_json_with_probe(item: Option<&Item>, probe: &dyn ClaudeProbe) -> String {
    let (id, project, title, repo, prompt, launchable, issues) = match item {
        Some(it) => {
            let p = new_launch_prompt(it);
            let l = it.launchable;
            let iss: Vec<serde_json::Value> =
                it.issues.iter().map(|s| serde_json::json!(s)).collect();
            (
                Some(it.id.clone()),
                it.project.clone(),
                it.session.clone(),
                it.repo.clone(),
                p,
                l,
                iss,
            )
        }
        None => (
            None,
            serde_json::Value::Null
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_default(),
            "pending-work claude check".to_string(),
            None,
            "Verify Claude Code session launch.".to_string(),
            true,
            vec![],
        ),
    };
    let command = new_claude_launch_command(&title, &prompt, probe.path());
    let display = get_claude_command_display(&title, &prompt);
    let result = if probe.available() && launchable {
        "pass"
    } else {
        "fail"
    };

    let mut map = serde_json::Map::new();
    if let Some(ref i) = id {
        map.insert("id".into(), serde_json::json!(i));
    }
    map.insert("project".into(), serde_json::json!(project));
    map.insert("session".into(), serde_json::json!(title));
    map.insert("title".into(), serde_json::json!(title));
    map.insert("repo".into(), serde_json::json!(repo));
    map.insert(
        "claude".into(),
        serde_json::json!({
            "available": probe.available(),
            "path": probe.path(),
            "version": probe.version()
        }),
    );
    map.insert("sessionTitleFlag".into(), serde_json::json!("--name"));
    map.insert("command".into(), serde_json::json!(command));
    map.insert("commandDisplay".into(), serde_json::json!(display));
    map.insert("launchable".into(), serde_json::json!(launchable));
    map.insert("result".into(), serde_json::json!(result));
    map.insert("issues".into(), serde_json::json!(issues));
    serde_json::to_string_pretty(&serde_json::Value::Object(map)).unwrap()
}

pub(super) fn invoke_claude_launch(
    cfg: &Config,
    id: &str,
    probe: &dyn ClaudeProbe,
    force: bool,
) -> Result<String, String> {
    // ! Notice on stderr keeps stdout machine-output clean.
    eprintln!("note: launch-claude emits a direct claude launch.");
    let item = find_pending_item(cfg, id)?;
    if !item.launchable {
        return Err(errors::not_launchable(&item.id, &item.issues));
    }
    if !probe.available() {
        return Err(format!(
            "Claude CLI not found on PATH; cannot launch. Run 'pwf pw verify --id {}' for details.",
            id
        ));
    }
    let title = item.session.clone();
    let prompt = new_launch_prompt(&item);
    let can_spawn = force || probe.interactive();
    if !can_spawn {
        let display = get_claude_command_display(&title, &prompt);
        let repo = item.repo.as_deref().unwrap_or(".");
        let mut out = String::from("No interactive TTY detected; not spawning Claude.\n");
        out.push_str(&format!("Run this in your terminal (cwd: {repo}):\n"));
        out.push_str(&format!("  {display}\n"));
        out.push_str("Or re-run with --force to spawn anyway.\n");
        return Ok(out);
    }
    // Real spawn — not exercised in tests; returned as informational text.
    let command = new_claude_launch_command(&title, &prompt, probe.path());
    let exe = &command[0];
    let rest = &command[1..];
    let repo = item.repo.as_deref().unwrap_or(".");
    let status = std::process::Command::new(exe)
        .args(rest)
        .current_dir(repo)
        .status()
        .map_err(|e| format!("Failed to spawn claude: {e}"))?;
    if !status.success() {
        return Err(format!("claude exited with status {}", status));
    }
    Ok(String::new())
}
