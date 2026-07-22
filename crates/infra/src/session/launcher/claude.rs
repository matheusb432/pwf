//! Encodes Claude Code launches.

use pwf_application::pending_work::session::AgentLaunch;

use super::{CLAUDE_BINARY, argv::LaunchArgv};

// FIXME: this silently ignores almost every parameter! bad abstraction, there must be a
// 'ClaudeLaunch' struct instead that clearly defines what it uses
pub(super) fn launch_argv(launch: &AgentLaunch) -> Vec<String> {
    let mut argv = LaunchArgv::new(CLAUDE_BINARY).flag("--name", launch.title.clone());
    if let Some(model) = &launch.model {
        argv = argv.flag("--model", model.clone());
    }
    argv.into_guarded(launch.prompt.clone())
}

#[cfg(test)]
mod tests {
    use pwf_application::pending_work::session::{Agent, AgentLaunch};

    use super::*;

    fn launch(model: Option<&str>) -> AgentLaunch {
        AgentLaunch {
            agent: Agent::Claude,
            task_id: "PWF-0038".to_string(),
            title: "PWF-0038 - zellij dispatches".to_string(),
            repository: "/repo/pwf".to_string(),
            prompt: "Pending-work ID: PWF-0038\nProject: pwf\n\ndo PWF-0038".to_string(),
            model: model.map(str::to_string),
        }
    }

    #[test]
    fn builds_named_argv_with_guard_and_prompt() {
        let argv = launch_argv(&launch(None));

        assert_eq!(
            argv,
            vec![
                "claude",
                "--name",
                "PWF-0038 - zellij dispatches",
                "--",
                "Pending-work ID: PWF-0038\nProject: pwf\n\ndo PWF-0038",
            ]
        );
    }

    #[test]
    fn appends_model_flag_when_present() {
        let argv = launch_argv(&launch(Some("sonnet")));

        assert_eq!(
            argv,
            vec![
                "claude",
                "--name",
                "PWF-0038 - zellij dispatches",
                "--model",
                "sonnet",
                "--",
                "Pending-work ID: PWF-0038\nProject: pwf\n\ndo PWF-0038",
            ]
        );
    }

    #[test]
    fn hostile_title_and_prompt_stay_inert_single_elements() {
        let mut prepared = launch(None);
        prepared.title = "--dangerously-skip-permissions".to_string();
        prepared.prompt = "; rm -rf ~ $(curl evil)\n--dangerously-skip-permissions".to_string();

        let argv = launch_argv(&prepared);

        assert_eq!(argv[0], "claude");
        assert_eq!(argv[1], "--name");
        assert_eq!(argv[2], "--dangerously-skip-permissions");
        assert_eq!(argv[3], "--");
        assert_eq!(argv[4], prepared.prompt);
        assert_eq!(argv.len(), 5);
    }
}
