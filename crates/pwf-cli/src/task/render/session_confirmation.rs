use pwf_models::session::DispatchMode;
use pwf_wire::task::session::DispatchConfirmation;

use super::agent_name;
use crate::confirmation::{ConfirmationDefault, ConfirmationDialog, ConfirmationTone, Detail};

const CURRENT_TERMINAL_TARGET: &str = "current terminal";
const UNKNOWN_CREATED: &str = "(unknown)";

pub(in crate::task) fn render_session_confirmation(
    confirmation: &DispatchConfirmation,
) -> ConfirmationDialog {
    let (mode, target) = match confirmation.mode {
        DispatchMode::Inline => ("inline", CURRENT_TERMINAL_TARGET.to_string()),
        DispatchMode::Multiplexer => (
            "tmux",
            format!("tmux session {}", confirmation.target().session_name()),
        ),
    };
    let details = [
        Detail::new("Task", &confirmation.task_id),
        Detail::new("Title", confirmation.title.to_string()),
        Detail::new(
            "Created",
            confirmation
                .created
                .as_ref()
                .map_or_else(|| UNKNOWN_CREATED.to_string(), ToString::to_string),
        ),
        Detail::new("Mode", mode),
        Detail::new("Agent", agent_name(confirmation.agent)),
        Detail::new("Model", confirmation.model.to_string()),
        Detail::new("Effort", confirmation.effort.to_string()),
        Detail::new("Autonomy", yes_no(confirmation.directives.autonomous)),
        Detail::new("Worktree", yes_no(confirmation.directives.worktree)),
        Detail::new("Prompt prefix", yes_no(confirmation.has_pushed_prompt)),
        Detail::new("Target", target),
    ];
    ConfirmationDialog::new(
        "Confirm session dispatch",
        details.into(),
        "Proceed with session dispatch?",
        ConfirmationDefault::Yes,
        ConfirmationTone::Informational,
    )
}

fn yes_no(enabled: bool) -> &'static str {
    if enabled { "yes" } else { "no" }
}

#[cfg(test)]
mod tests {
    use pwf_models::{
        session::{Agent, AgentModel, DispatchMode, LaunchDirectives, SessionEffort},
        task::{TaskId, TaskTitle},
    };
    use pwf_wire::task::{TaskHeading, session::DispatchConfirmation};

    use super::*;

    #[test]
    fn question_renders_dispatch_context_metadata_without_the_prompt_body() {
        let confirmation = DispatchConfirmation {
            task_id: TaskId::try_new("PWF-0001").unwrap(),
            title: TaskHeading::Title(TaskTitle::try_new("dispatch me").unwrap()),
            created: Some("2026-07-01".parse().unwrap()),
            mode: DispatchMode::Inline,
            agent: Agent::Codex,
            directives: LaunchDirectives {
                worktree: true,
                autonomous: true,
            },
            has_pushed_prompt: true,
            model: AgentModel::default(),
            effort: SessionEffort::XHigh,
        };

        let out = render_session_confirmation(&confirmation).render(false);

        assert_eq!(
            out,
            "Confirm session dispatch\n\n  Task           PWF-0001\n  Title          dispatch me\n  Created        2026-07-01\n  Mode           inline\n  Agent          codex\n  Model          default\n  Effort         xhigh\n  Autonomy       yes\n  Worktree       yes\n  Prompt prefix  yes\n  Target         current terminal"
        );
    }

    #[test]
    fn tmux_mode_targets_the_named_session_and_falls_back_on_missing_created() {
        let confirmation = DispatchConfirmation {
            task_id: TaskId::try_new("PWF-0001").unwrap(),
            title: TaskHeading::Title(TaskTitle::try_new("dispatch me").unwrap()),
            created: None,
            mode: DispatchMode::Multiplexer,
            agent: Agent::Claude,
            directives: LaunchDirectives::default(),
            has_pushed_prompt: false,
            model: AgentModel::default(),
            effort: SessionEffort::High,
        };

        let out = render_session_confirmation(&confirmation).render(false);

        assert_eq!(
            out,
            "Confirm session dispatch\n\n  Task           PWF-0001\n  Title          dispatch me\n  Created        (unknown)\n  Mode           tmux\n  Agent          claude\n  Model          default\n  Effort         high\n  Autonomy       no\n  Worktree       no\n  Prompt prefix  no\n  Target         tmux session pwf"
        );
    }
}
