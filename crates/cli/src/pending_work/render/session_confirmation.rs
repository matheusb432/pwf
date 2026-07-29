use pwf_application::pending_work::session::{DispatchConfirmation, DispatchMode};

use super::agent_name;
use crate::confirm_prompt::{ConfirmationPrompt, Field};

const CURRENT_TERMINAL_TARGET: &str = "current terminal";
const UNKNOWN_CREATED: &str = "(unknown)";

pub(in crate::pending_work) fn render_session_confirmation(
    confirmation: &DispatchConfirmation,
) -> String {
    let (mode, target) = match confirmation.mode {
        DispatchMode::Inline => ("inline", CURRENT_TERMINAL_TARGET.to_string()),
        DispatchMode::Multiplexer => (
            "tmux",
            format!("tmux session {}", confirmation.target.session),
        ),
    };
    let fields = [
        Field::new("task_id", confirmation.task_id.clone()),
        Field::new("title", confirmation.title.clone()),
        Field::new(
            "created",
            confirmation
                .created
                .clone()
                .unwrap_or_else(|| UNKNOWN_CREATED.to_string()),
        ),
        Field::new("mode", mode),
        Field::new("agent", agent_name(confirmation.agent)),
        Field::new("model", confirmation.model.clone()),
        Field::new("effort", confirmation.effort.to_string()),
        Field::new(
            "autonomy",
            Enabled::from(confirmation.directives.autonomous).label(),
        ),
        Field::new(
            "worktree",
            Enabled::from(confirmation.directives.worktree).label(),
        ),
        Field::new("target", target),
    ];
    ConfirmationPrompt::new(
        "Confirm session dispatch",
        &fields,
        "Proceed with session dispatch?",
    )
    .to_string()
}

#[derive(Clone, Copy)]
enum Enabled {
    Yes,
    No,
}

impl Enabled {
    fn label(self) -> &'static str {
        match self {
            Enabled::Yes => "yes",
            Enabled::No => "no",
        }
    }
}

impl From<bool> for Enabled {
    fn from(value: bool) -> Self {
        if value { Enabled::Yes } else { Enabled::No }
    }
}

#[cfg(test)]
mod tests {
    use pwf_application::pending_work::session::{
        Agent, DispatchConfirmation, DispatchMode, DispatchTarget, LaunchDirectives, SessionEffort,
    };

    use super::*;

    #[test]
    fn question_renders_dispatch_context_metadata_without_the_prompt_body() {
        let confirmation = DispatchConfirmation {
            task_id: "PWF-0001".to_string(),
            title: "dispatch me".to_string(),
            created: Some("2026-07-01".to_string()),
            mode: DispatchMode::Inline,
            agent: Agent::Codex,
            directives: LaunchDirectives {
                worktree: true,
                autonomous: true,
            },
            target: DispatchTarget {
                session: "pwf".to_string(),
                window: "PWF-0001".to_string(),
            },
            model: String::default(),
            effort: SessionEffort::XHigh,
        };

        let out = render_session_confirmation(&confirmation);

        assert!(out.contains("# Confirm session dispatch"));
        assert!(out.contains("task_id: PWF-0001"));
        assert!(out.contains("title: dispatch me"));
        assert!(out.contains("created: 2026-07-01"));
        assert!(out.contains("mode: inline"));
        assert!(out.contains("agent: codex"));
        assert!(out.contains("effort: xhigh"));
        assert!(out.contains("autonomy: yes"));
        assert!(out.contains("worktree: yes"));
        assert!(out.contains("target: current terminal"));
    }

    #[test]
    fn tmux_mode_targets_the_named_session_and_falls_back_on_missing_created() {
        let confirmation = DispatchConfirmation {
            task_id: "PWF-0001".to_string(),
            title: "dispatch me".to_string(),
            created: None,
            mode: DispatchMode::Multiplexer,
            agent: Agent::Claude,
            directives: LaunchDirectives::default(),
            target: DispatchTarget {
                session: "pwf".to_string(),
                window: "PWF-0001".to_string(),
            },
            model: String::default(),
            effort: SessionEffort::High,
        };

        let out = render_session_confirmation(&confirmation);

        assert!(out.contains("mode: tmux"));
        assert!(out.contains("target: tmux session pwf"));
        assert!(out.contains("autonomy: no"));
        assert!(out.contains("effort: high"));
        assert!(out.contains("worktree: no"));
        assert!(out.contains("created: (unknown)"));
    }
}
