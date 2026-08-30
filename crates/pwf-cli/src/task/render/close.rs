//! Renders byte-stable close and reopen confirmations plus ordered done-queue diagnostics.

use pwf_client::v1::{ClosedTask, ReopenTaskResult, reopen_task_result};

use super::confirmation::render_review_task;

pub(in crate::task) fn render_cancelled(outcome: &ClosedTask) -> String {
    render_closed("Cancelled", outcome)
}

pub(in crate::task) fn render_completed(outcome: &ClosedTask) -> String {
    render_closed("Done", outcome)
}

fn render_closed(action: &str, outcome: &ClosedTask) -> String {
    let mut text = format!(
        "{} {} ({} :: {})\n",
        action, outcome.id, outcome.project, outcome.title
    );
    if let Some(review) = outcome.review_task.as_ref() {
        text.push_str(&render_review_task(review));
    }
    text
}

/// Renders the byte-stable reopen or already-active confirmation.
pub(in crate::task) fn render_reopened(task_id: &str, outcome: &ReopenTaskResult) -> String {
    match outcome.outcome.as_ref() {
        Some(reopen_task_result::Outcome::AlreadyActive(task)) => {
            format!(
                "{} is already active ({}), so it was skipped.\n",
                task.id, task.project
            )
        }
        Some(reopen_task_result::Outcome::Reopened(task)) => {
            format!("Reopened {} ({})\n", task.id, task.project)
        }
        Some(reopen_task_result::Outcome::Aborted(task)) => {
            format!("# reopen {}: aborted\nnothing changed.\n", task.id)
        }
        None => {
            format!("# reopen {task_id}: invalid server response\n")
        }
    }
}

pub(in crate::task) fn emit_cancel_diagnostics(outcome: &ClosedTask) {
    emit_close_diagnostics(outcome);
}

pub(in crate::task) fn emit_complete_diagnostics(outcome: &ClosedTask) {
    emit_close_diagnostics(outcome);
}

fn emit_close_diagnostics(outcome: &ClosedTask) {
    if let Some(project) = outcome.futuro_renamed_project.as_deref() {
        eprintln!("info: normalized `## Futuro` header to `## Future` in {project}");
    }
    if !outcome.evicted_ids.is_empty() {
        eprintln!(
            "info: archived {} done task(s) past the section cap: {}",
            outcome.evicted_ids.len(),
            outcome
                .evicted_ids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}
