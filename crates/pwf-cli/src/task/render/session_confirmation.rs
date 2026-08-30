use pwf_client::pb::{Agent, DispatchConfirmation, DispatchMode, SessionEffort};

use super::{
    agent_name,
    session::{effort_name, session_name},
};
use crate::confirmation::{ConfirmationDefault, ConfirmationDialog, ConfirmationTone, Detail};

const CURRENT_TERMINAL_TARGET: &str = "current terminal";
const UNKNOWN_CREATED: &str = "(unknown)";

pub(in crate::task) fn render_session_confirmation(
    confirmation: &DispatchConfirmation,
) -> ConfirmationDialog {
    let identity = &confirmation.session_name;
    let is_compound = confirmation.task_ids.len() > 1;
    let mode = DispatchMode::try_from(confirmation.mode).unwrap_or(DispatchMode::Unspecified);
    let (mode_name, target) = match mode {
        DispatchMode::Inline => ("inline", CURRENT_TERMINAL_TARGET.to_string()),
        DispatchMode::Multiplexer => (
            "tmux",
            format!(
                "tmux session {}",
                confirmation
                    .task_ids
                    .first()
                    .map_or_else(|| "(unknown)".to_string(), |task_id| session_name(task_id))
            ),
        ),
        DispatchMode::Unspecified => ("unspecified", "unknown target".to_string()),
    };
    let directives = confirmation.directives.as_ref();
    let agent = Agent::try_from(confirmation.agent).unwrap_or(Agent::Unspecified);
    let effort = SessionEffort::try_from(confirmation.effort).unwrap_or(SessionEffort::Unspecified);
    let mut details = vec![Detail::new(
        if is_compound { "Tasks" } else { "Task" },
        identity,
    )];
    if !is_compound {
        details.extend([
            Detail::new("Title", &confirmation.title),
            Detail::new(
                "Created",
                confirmation.created.as_deref().unwrap_or(UNKNOWN_CREATED),
            ),
        ]);
    }
    details.extend([
        Detail::new("Mode", mode_name),
        Detail::new("Agent", agent_name(agent)),
        Detail::new("Model", confirmation.model.as_deref().unwrap_or("default")),
        Detail::new("Effort", effort_name(effort)),
        Detail::new(
            "Autonomy",
            yes_no(directives.is_some_and(|directives| directives.autonomous)),
        ),
        Detail::new(
            "Worktree",
            yes_no(directives.is_some_and(|directives| directives.worktree)),
        ),
        Detail::new("Prompt prefix", yes_no(confirmation.has_pushed_prompt)),
        Detail::new("Target", target),
    ]);
    ConfirmationDialog::new(
        "Confirm session dispatch",
        details,
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
    use pwf_client::pb::LaunchDirectives;

    use super::*;

    #[test]
    fn question_renders_dispatch_context_metadata_without_the_prompt_body() {
        let confirmation = DispatchConfirmation {
            task_ids: vec!["FOO-0001".to_string()],
            title: "dispatch me".to_string(),
            created: Some("2026-07-01".to_string()),
            mode: DispatchMode::Inline as i32,
            agent: Agent::Codex as i32,
            directives: Some(LaunchDirectives {
                worktree: true,
                autonomous: true,
            }),
            has_pushed_prompt: true,
            model: None,
            effort: SessionEffort::Xhigh as i32,
            session_name: "FOO-0001".to_string(),
        };
        let out = render_session_confirmation(&confirmation).render(false);
        assert_eq!(
            out,
            "Confirm session dispatch\n\n  Task           FOO-0001\n  Title          dispatch me\n  Created        2026-07-01\n  Mode           inline\n  Agent          codex\n  Model          default\n  Effort         xhigh\n  Autonomy       yes\n  Worktree       yes\n  Prompt prefix  yes\n  Target         current terminal"
        );
    }

    #[test]
    fn tmux_mode_targets_the_named_session_and_falls_back_on_missing_created() {
        let confirmation = DispatchConfirmation {
            task_ids: vec!["FOO-0001".to_string()],
            title: "dispatch me".to_string(),
            created: None,
            mode: DispatchMode::Multiplexer as i32,
            agent: Agent::Claude as i32,
            directives: Some(LaunchDirectives::default()),
            has_pushed_prompt: false,
            model: None,
            effort: SessionEffort::High as i32,
            session_name: "FOO-0001".to_string(),
        };
        let out = render_session_confirmation(&confirmation).render(false);
        assert_eq!(
            out,
            "Confirm session dispatch\n\n  Task           FOO-0001\n  Title          dispatch me\n  Created        (unknown)\n  Mode           tmux\n  Agent          claude\n  Model          default\n  Effort         high\n  Autonomy       no\n  Worktree       no\n  Prompt prefix  no\n  Target         tmux session foo"
        );
    }

    #[test]
    fn compound_confirmation_uses_the_sorted_session_identity() {
        let confirmation = DispatchConfirmation {
            task_ids: vec!["FOO-0023".to_string(), "FOO-0015".to_string()],
            title: "first supplied task".to_string(),
            created: Some("2026-07-01".to_string()),
            mode: DispatchMode::Multiplexer as i32,
            agent: Agent::Codex as i32,
            directives: Some(LaunchDirectives::default()),
            has_pushed_prompt: false,
            model: None,
            effort: SessionEffort::High as i32,
            session_name: "foo15,foo23".to_string(),
        };

        let out = render_session_confirmation(&confirmation).render(false);

        assert!(out.contains("Tasks          foo15,foo23"), "{out}");
        assert!(!out.contains("first supplied task"), "{out}");
        assert!(out.contains("Target         tmux session foo"), "{out}");
    }
}
