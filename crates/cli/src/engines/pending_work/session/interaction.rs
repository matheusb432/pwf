use pwf_application::pending_work::session::{DispatchConfirmation, SessionInteraction};

use super::confirmation;
use crate::confirm::{Confirmation, DefaultAnswer};

#[derive(Debug, Clone, Copy, Default)]
pub(in crate::engines::pending_work) struct CliSessionInteraction;

impl SessionInteraction for CliSessionInteraction {
    fn warn_agent_missing(&self, binary: &str) {
        eprintln!(
            "note: {binary} not found on PATH from here; the agent will surface the error if it can't run."
        );
    }

    fn confirm(&self, context: &DispatchConfirmation) -> bool {
        matches!(
            crate::confirm::terminal(&confirmation::render(context), DefaultAnswer::Yes),
            Confirmation::Accepted | Confirmation::NonInteractive
        )
    }

    fn inline_starting(&self, task_id: &str, repository: &str) {
        eprintln!("running {task_id} inline in {repository}…");
    }
}
