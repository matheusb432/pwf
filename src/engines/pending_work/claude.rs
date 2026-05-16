// Claude CLI probe, command construction, verify output, and the direct
// claude launch.

use super::errors::PendingWorkError;
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

/// Render the verify result as markdown given an optional item and a probe.
/// The `pass`/`fail` token sits in the heading so an agent can branch on one read.
pub fn verify_text_with_probe(item: Option<&Item>, probe: &dyn ClaudeProbe) -> String {
    let (id, title, prompt, launchable, issues): (Option<&str>, &str, String, bool, Vec<String>) =
        match item {
            Some(it) => (
                Some(it.id.as_str()),
                it.session.as_str(),
                new_launch_prompt(it),
                it.launchable,
                it.issues.clone(),
            ),
            None => (
                None,
                "pending-work claude check",
                "Verify Claude Code session launch.".to_string(),
                true,
                vec![],
            ),
        };
    let display = get_claude_command_display(title, &prompt);
    let result = if probe.available() && launchable {
        "pass"
    } else {
        "fail"
    };

    let mut out = match id {
        Some(i) => format!("# verify {i} \u{2014} {result}\n"),
        None => format!("# verify \u{2014} {result}\n"),
    };
    if probe.available() {
        let ver = probe
            .version()
            .map(|v| format!(" ({v})"))
            .unwrap_or_default();
        let path = probe.path().map(|p| format!(" at {p}")).unwrap_or_default();
        out.push_str(&format!("claude: available{ver}{path}\n"));
    } else {
        out.push_str("claude: not found on PATH\n");
    }
    out.push_str(&format!(
        "launchable: {}\n",
        if launchable { "yes" } else { "no" }
    ));
    out.push_str(&format!("command: {display}\n"));
    if issues.is_empty() {
        out.push_str("issues: none\n");
    } else {
        out.push_str("issues:\n");
        for iss in &issues {
            out.push_str(&format!("- {iss}\n"));
        }
    }
    out
}

pub(super) fn invoke_claude_launch(
    cfg: &Config,
    id: &str,
    probe: &dyn ClaudeProbe,
    force: bool,
) -> Result<String, PendingWorkError> {
    // ! Notice on stderr keeps stdout machine-output clean.
    eprintln!("note: launch-claude emits a direct claude launch.");
    let item = find_pending_item(cfg, id)?;
    if !item.launchable {
        return Err(PendingWorkError::NotLaunchable {
            id: item.id,
            issues: item.issues,
        });
    }
    if !probe.available() {
        return Err(PendingWorkError::ClaudeNotFound { id: id.to_string() });
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
        .map_err(|source| PendingWorkError::FailedToSpawnClaude { source })?;
    if !status.success() {
        return Err(PendingWorkError::ClaudeExited { status });
    }
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::pending_work::errors::PendingWorkError;
    use std::fs;

    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    fn cfg_with_item() -> (std::path::PathBuf, Config) {
        let stage = std::env::temp_dir().join(format!("pwf_claude_{}", nanos()));
        let notes = stage.join("notes");
        let project = notes.join("pwf");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("PWF-0001.md"),
            "---\nstatus: active\ntitle: cli launch\nproject: pwf\ncreated: 2026-01-01\n---\n\nlaunch claude\n",
        )
        .unwrap();
        fs::write(project.join("pwf.md"), "- [[PWF-0001|cli launch]]\n").unwrap();
        let cfg = crate::config::from_json(
            &format!(
                r#"{{ "notesDir": "{}", "projects": {{ "pwf": "{}" }}, "prefixes": {{ "pwf": "PWF" }} }}"#,
                notes.to_string_lossy().replace('\\', "\\\\"),
                stage.to_string_lossy().replace('\\', "\\\\")
            ),
            None,
        )
        .unwrap();
        (stage, cfg)
    }

    #[test]
    fn verify_text_pass_has_result_in_heading() {
        let probe = FakeProbe {
            available: true,
            path: Some("/usr/bin/claude".to_string()),
            version: Some("1.2.3".to_string()),
            interactive: true,
        };
        let item = Item {
            launchable: true,
            issues: vec![],
            ..crate::engines::pending_work::model::Item::default_for_test("PWF-0001", "cli launch")
        };
        let out = verify_text_with_probe(Some(&item), &probe);
        assert!(out.starts_with("# verify PWF-0001 \u{2014} pass"));
        assert!(out.contains("claude: available (1.2.3) at /usr/bin/claude"));
        assert!(out.contains("launchable: yes"));
        assert!(out.contains("command: "));
        assert!(out.contains("issues: none"));
    }

    #[test]
    fn verify_text_fail_lists_issues() {
        let probe = FakeProbe {
            available: false,
            path: None,
            version: None,
            interactive: false,
        };
        let item = Item {
            launchable: false,
            issues: vec!["Prompt is a placeholder".to_string()],
            ..crate::engines::pending_work::model::Item::default_for_test("PWF-0002", "broken")
        };
        let out = verify_text_with_probe(Some(&item), &probe);
        assert!(out.starts_with("# verify PWF-0002 \u{2014} fail"));
        assert!(out.contains("claude: not found on PATH"));
        assert!(out.contains("issues:\n"));
        assert!(out.contains("- Prompt is a placeholder"));
    }

    #[test]
    fn verify_text_available_claude_non_launchable_item_fails() {
        let probe = FakeProbe {
            available: true,
            path: Some("/usr/bin/claude".to_string()),
            version: Some("1.2.3".to_string()),
            interactive: true,
        };
        let item = Item {
            launchable: false,
            issues: vec!["Prompt is a placeholder".to_string()],
            ..crate::engines::pending_work::model::Item::default_for_test(
                "PWF-0003",
                "non-launchable",
            )
        };
        let out = verify_text_with_probe(Some(&item), &probe);
        assert!(out.starts_with("# verify PWF-0003 \u{2014} fail"));
        assert!(out.contains("launchable: no"));
        assert!(out.contains("command: "));
    }

    #[test]
    fn failed_claude_spawn_returns_typed_error_with_legacy_display() {
        let (_stage, cfg) = cfg_with_item();
        let probe = FakeProbe {
            available: true,
            path: Some("/definitely/not/claude".to_string()),
            version: None,
            interactive: true,
        };

        let err = invoke_claude_launch(&cfg, "PWF-0001", &probe, false).unwrap_err();

        assert!(matches!(err, PendingWorkError::FailedToSpawnClaude { .. }));
        assert!(err.to_string().starts_with("Failed to spawn claude: "));
    }
}
