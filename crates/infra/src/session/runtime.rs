//! Implements the application session runtime with host processes and filesystems.

use std::process::Command;

use pwf_application::pending_work::session::{
    Agent, AgentLaunch, AgentProbe, DispatchTarget, SessionRuntime, TabOpenError,
};

use super::{inline, launcher, zellij};

/// Runs prepared session launches against host processes and filesystems.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessSessionRuntime;

impl SessionRuntime for ProcessSessionRuntime {
    fn probe_agent(&self, agent: Agent) -> AgentProbe {
        probe_agent(agent)
    }

    fn repository_is_directory(&self, path: &str) -> bool {
        std::path::Path::new(path).is_dir()
    }

    fn multiplexer_available(&self) -> bool {
        zellij::available()
    }

    fn command_preview(&self, launch: &AgentLaunch) -> String {
        command_preview(&launcher::launch_argv(launch))
    }

    fn run_inline(&self, launch: &AgentLaunch) -> Result<(), String> {
        inline::run(&launcher::launch_argv(launch), &launch.repository)
    }

    fn open_tab(&self, target: &DispatchTarget, launch: &AgentLaunch) -> Result<(), TabOpenError> {
        zellij::open_tab(
            &target.session,
            &launch.repository,
            &target.tab,
            &launcher::launch_argv(launch),
        )
    }

    fn ensure_session(&self, session: &str) -> Result<(), String> {
        zellij::ensure_session(session)
    }
}

fn probe_agent(agent: Agent) -> AgentProbe {
    let binary = launcher::binary(agent);
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

fn command_preview(argv: &[String]) -> String {
    let Some((last, head)) = argv.split_last() else {
        return String::new();
    };
    let mut parts: Vec<String> = head
        .iter()
        .map(|argument| {
            if argument.contains(' ') {
                format!("\"{argument}\"")
            } else {
                argument.clone()
            }
        })
        .collect();
    let first = last.lines().next().unwrap_or("");
    let shown = if last.lines().nth(1).is_some() {
        format!("{first}\u{2026}")
    } else {
        first.to_string()
    };
    parts.push(format!("\"{shown}\""));
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use pwf_application::pending_work::session::Agent;

    use super::*;

    fn launch(agent: Agent) -> AgentLaunch {
        AgentLaunch {
            agent,
            task_id: "PWF-0068".to_string(),
            title: "PWF-0068 - codex verify".to_string(),
            repository: "/repo".to_string(),
            prompt: "Pending-work ID: PWF-0068\nProject: pwf\n\ndo PWF-0068".to_string(),
            model: None,
        }
    }

    #[test]
    fn missing_agent_probe_retains_the_selected_binary() {
        let probe = probe_agent(Agent::Codex);

        assert_eq!(probe.binary, "codex");
    }

    #[test]
    fn which_binary_resolves_a_real_program_and_rejects_a_fake() {
        #[cfg(unix)]
        assert!(which_binary("sh").is_some());
        assert!(which_binary("definitely-not-a-real-binary-xyz").is_none());
    }

    #[test]
    fn claude_preview_preserves_model_guard_and_prompt_position() {
        let runtime = ProcessSessionRuntime;
        let mut prepared = launch(Agent::Claude);
        prepared.model = Some("opus".to_string());

        let preview = runtime.command_preview(&prepared);

        assert!(preview.starts_with("claude --name \"PWF-0068 - codex verify\" --model opus -- "));
        assert!(preview.ends_with("\"Pending-work ID: PWF-0068…\""));
    }

    #[test]
    fn codex_preview_contains_the_hidden_shim_and_guarded_prompt() {
        let runtime = ProcessSessionRuntime;

        let preview = runtime.command_preview(&launch(Agent::Codex));

        assert!(preview.contains("__codex-thread-title"));
        assert!(preview.contains(" codex -- "));
        assert!(preview.ends_with("\"Pending-work ID: PWF-0068…\""));
    }

    #[test]
    fn empty_preview_is_empty() {
        assert_eq!(command_preview(&[]), "");
    }
}
