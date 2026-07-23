//! Owns concrete provider probes, launch preparation, and command previews.

mod argv;
mod claude;
mod codex;

use std::process::Command;

pub use claude::ClaudeHarness;
pub use codex::CodexHarness;
pub use pwf_application::pending_work::session::AgentProbe;

fn probe(binary: &str) -> AgentProbe {
    let (available, path, version) = match which_binary(binary) {
        None => (false, None, None),
        Some(path) => {
            let version = run_version(&path);
            (true, Some(path), version)
        }
    };
    AgentProbe {
        binary: binary.to_string(),
        available,
        path,
        version,
    }
}

fn which_binary(name: &str) -> Option<String> {
    #[cfg(windows)]
    {
        let output = Command::new("where").arg(name).output().ok()?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            return stdout.lines().next().map(|line| line.trim().to_string());
        }
        None
    }
    #[cfg(not(windows))]
    {
        let output = Command::new("sh")
            .args(["-c", &format!("command -v {name}")])
            .output()
            .ok()?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let first = stdout.lines().next()?.trim();
            if !first.is_empty() {
                return Some(first.to_string());
            }
        }
        None
    }
}

fn run_version(path: &str) -> Option<String> {
    let output = Command::new(path).arg("--version").output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().next().map(|line| line.trim().to_string())
}

/// Renders complete argv as a shell-safe command preview.
#[must_use]
pub fn render_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|argument| shell_words::quote(argument))
        .collect::<Vec<_>>()
        .join(" ")
}
