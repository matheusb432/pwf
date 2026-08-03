//! Renders byte-stable close and reopen confirmations plus ordered done-queue diagnostics.

use pwf_application::task::{complete_task::CompleteTaskOk, reopen_task::ReopenTaskOk};

use super::confirmation::render_review_task;

/// Renders the byte-stable close confirmation and optional review-task allocation.
pub(in crate::task) fn render_closed(outcome: &CompleteTaskOk) -> String {
    let mut text = format!(
        "{} {} ({} :: {})\n",
        outcome.action.past_tense(),
        outcome.id.as_ref(),
        outcome.project.as_ref(),
        outcome.title
    );
    if let Some(review) = &outcome.review_task {
        text.push_str(&render_review_task(review));
    }
    text
}

/// Renders the byte-stable reopen or already-active confirmation.
pub(in crate::task) fn render_reopened(outcome: &ReopenTaskOk) -> String {
    if outcome.already_active {
        format!(
            "{} already active ({}) — skipped\n",
            outcome.id.as_ref(),
            outcome.project.as_ref()
        )
    } else {
        format!(
            "Reopened {} ({})\n",
            outcome.id.as_ref(),
            outcome.project.as_ref()
        )
    }
}

/// Emits header-normalization before section-cap-eviction diagnostics on stderr.
pub(in crate::task) fn emit_close_diagnostics(outcome: &CompleteTaskOk) {
    if let Some(project) = &outcome.futuro_renamed_project {
        eprintln!(
            "info: normalized `## Futuro` header to `## Future` in {}",
            project.as_ref()
        );
    }
    if !outcome.evicted_ids.is_empty() {
        eprintln!(
            "info: archived {} done task(s) past the section cap: {}",
            outcome.evicted_ids.len(),
            outcome
                .evicted_ids
                .iter()
                .map(pwf_models::task::TaskId::as_ref)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}
