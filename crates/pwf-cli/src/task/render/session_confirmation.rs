use pwf_client::pb::{Agent, DispatchConfirmation, SessionEffort};

use super::{agent_name, session::effort_name};
use crate::confirmation::{ConfirmationDefault, ConfirmationDialog, ConfirmationTone, Detail};

const UNKNOWN_CREATED: &str = "(unknown)";

pub(in crate::task) fn render_session_confirmation(
    confirmation: &DispatchConfirmation,
) -> ConfirmationDialog {
    let identity = &confirmation.session_name;
    let is_compound = confirmation.task_ids.len() > 1;
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
        Detail::new("Agent", agent_name(agent)),
        Detail::new("Model", confirmation.model.as_deref().unwrap_or("default")),
        Detail::new("Effort", effort_name(effort)),
        Detail::new("Prompt prefix", yes_no(confirmation.has_pushed_prompt)),
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
    use super::*;

    #[test]
    fn question_renders_dispatch_context_metadata_without_the_prompt_body() {
        let confirmation = DispatchConfirmation {
            task_ids: vec!["FOO-0001".to_string()],
            title: "dispatch me".to_string(),
            created: Some("2026-07-01".to_string()),
            agent: Agent::Codex as i32,
            has_pushed_prompt: true,
            model: None,
            effort: SessionEffort::Xhigh as i32,
            session_name: "FOO-0001".to_string(),
        };
        let out = render_session_confirmation(&confirmation).render(false);
        assert_eq!(
            out,
            "Confirm session dispatch\n\n  Task           FOO-0001\n  Title          dispatch me\n  Created        2026-07-01\n  Agent          codex\n  Model          default\n  Effort         xhigh\n  Prompt prefix  yes"
        );
    }

    #[test]
    fn compound_confirmation_uses_the_sorted_session_identity() {
        let confirmation = DispatchConfirmation {
            task_ids: vec!["FOO-0023".to_string(), "FOO-0015".to_string()],
            title: "first supplied task".to_string(),
            created: Some("2026-07-01".to_string()),
            agent: Agent::Codex as i32,
            has_pushed_prompt: false,
            model: None,
            effort: SessionEffort::High as i32,
            session_name: "foo15,foo23".to_string(),
        };

        let out = render_session_confirmation(&confirmation).render(false);

        assert!(out.contains("Tasks          foo15,foo23"), "{out}");
        assert!(!out.contains("first supplied task"), "{out}");
    }
}
