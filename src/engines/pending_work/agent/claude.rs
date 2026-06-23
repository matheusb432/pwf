// Claude CLI probe, command construction, and verify output.

use crate::engines::pending_work::model::Item;
use crate::engines::pending_work::session::{AgentLauncher, ClaudeLauncher};

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
    #[cfg(windows)]
    {
        let output = std::process::Command::new("where")
            .arg("claude")
            .output()
            .ok()?;
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout);
            return s.lines().next().map(|l| l.trim().to_string());
        }
        None
    }
    #[cfg(not(windows))]
    {
        // `command -v` is a shell builtin → run it under `sh`. Prints the resolved
        // path on success; uses pwf's inherited PATH (best-effort preflight).
        let output = std::process::Command::new("sh")
            .args(["-c", "command -v claude"])
            .output()
            .ok()?;
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout);
            let first = s.lines().next()?.trim();
            if !first.is_empty() {
                return Some(first.to_string());
            }
        }
        None
    }
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

/// Render an agent argv (`[program, "--name", <title>, <prompt>]`, the shape
/// `ClaudeLauncher::argv` emits) as a single shell-ish command line, truncating
/// the trailing multi-line prompt to its first line so verify stays one screen.
fn command_line(argv: &[String]) -> String {
    match argv {
        [program, name_flag, title, prompt] => {
            let first = prompt.lines().next().unwrap_or("");
            let shown = if prompt.lines().nth(1).is_some() {
                format!("{first}…")
            } else {
                first.to_string()
            };
            format!("{program} {name_flag} \"{title}\" \"{shown}\"")
        }
        _ => argv.join(" "),
    }
}

/// Render the verify result as markdown given an optional item and a probe.
/// The `pass`/`fail` token sits in the heading so an agent can branch on one read.
/// The `command:` line is sourced from the real launcher (`ClaudeLauncher::argv`)
/// so it always reflects what `pwf session` would actually run.
pub fn verify_text_with_probe(item: Option<&Item>, probe: &dyn ClaudeProbe) -> String {
    let (id, launchable, issues, argv) = match item {
        Some(it) => (
            Some(it.id.as_str()),
            it.launchable,
            it.issues.clone(),
            ClaudeLauncher.argv(it),
        ),
        None => (
            None,
            true,
            vec![],
            vec![
                "claude".to_string(),
                "--name".to_string(),
                "pending-work claude check".to_string(),
                "Verify Claude Code session launch.".to_string(),
            ],
        ),
    };
    let display = command_line(&argv);
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
