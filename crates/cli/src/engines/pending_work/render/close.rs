//! Renders byte-stable close and reopen confirmations plus ordered done-queue diagnostics.

use pwf_application::pending_work::{done::CompletedPendingWork, reopen::ReopenedPendingWork};

use super::confirmation::render_review_item;

/// Renders the byte-stable close confirmation and optional review-task allocation.
pub(in crate::engines::pending_work) fn render_closed(outcome: &CompletedPendingWork) -> String {
    let mut text = format!(
        "{} {} ({} :: {})\n",
        outcome.action.past_tense(),
        outcome.id.as_ref(),
        outcome.project.as_ref(),
        outcome.title
    );
    if let Some(review) = &outcome.review_item {
        text.push_str(&render_review_item(review));
    }
    text
}

/// Renders the byte-stable reopen or already-active confirmation.
pub(in crate::engines::pending_work) fn render_reopened(outcome: &ReopenedPendingWork) -> String {
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
pub(in crate::engines::pending_work) fn emit_close_diagnostics(outcome: &CompletedPendingWork) {
    if let Some(project) = &outcome.futuro_renamed_project {
        eprintln!(
            "info: normalized `## Futuro` header to `## Future` in {}",
            project.as_ref()
        );
    }
    if !outcome.evicted_ids.is_empty() {
        eprintln!(
            "info: archived {} done item(s) past the section cap: {}",
            outcome.evicted_ids.len(),
            outcome
                .evicted_ids
                .iter()
                .map(pwf_domain::pending_work::WorkItemId::as_ref)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}
