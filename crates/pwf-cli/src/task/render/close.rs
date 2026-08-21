//! Renders byte-stable close and reopen confirmations plus ordered done-queue diagnostics.

use pwf_client::v1::{ClosedTask, ClosedTaskAction, ReopenedTask, ReopenedTaskOutcome};

use super::confirmation::render_review_task;

/// Renders the byte-stable close confirmation and optional review-task allocation.
pub(in crate::task) fn render_closed(outcome: &ClosedTask) -> String {
    let mut text = format!(
        "{} {} ({} :: {})\n",
        closed_action(outcome.action),
        outcome.id,
        outcome.project,
        outcome.title
    );
    if let Some(review) = &outcome.review_task {
        text.push_str(&render_review_task(review));
    }
    text
}

/// Renders the byte-stable reopen or already-active confirmation.
pub(in crate::task) fn render_reopened(outcome: &ReopenedTask) -> String {
    match ReopenedTaskOutcome::try_from(outcome.outcome).ok() {
        Some(ReopenedTaskOutcome::AlreadyActive) => {
            format!(
                "{} is already active ({}), so it was skipped.\n",
                outcome.id,
                outcome.project.as_deref().unwrap_or("unknown project")
            )
        }
        Some(ReopenedTaskOutcome::Reopened) => {
            format!(
                "Reopened {} ({})\n",
                outcome.id,
                outcome.project.as_deref().unwrap_or("unknown project")
            )
        }
        Some(ReopenedTaskOutcome::Aborted) => {
            format!("# reopen {}: aborted\nnothing changed.\n", outcome.id)
        }
        Some(ReopenedTaskOutcome::Unspecified) | None => {
            format!("# reopen {}: invalid server response\n", outcome.id)
        }
    }
}

fn closed_action(value: i32) -> &'static str {
    match ClosedTaskAction::try_from(value).ok() {
        Some(ClosedTaskAction::Done) => "Done",
        Some(ClosedTaskAction::Cancelled) => "Cancelled",
        Some(ClosedTaskAction::Unspecified) | None => "Closed",
    }
}

/// Emits header-normalization before section-cap-eviction diagnostics on stderr.
pub(in crate::task) fn emit_close_diagnostics(outcome: &ClosedTask) {
    if let Some(project) = &outcome.futuro_renamed_project {
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
