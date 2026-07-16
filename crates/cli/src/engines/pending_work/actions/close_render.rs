//! CLI rendering for the close (`done`/`cancel`) and `reopen` outcomes, plus
//! the stderr diagnostics for done-queue side effects. These strings were
//! formerly emitted from the application/infra layers (PWF-0123 error/rendering
//! relocation); every format here is verbatim from its prior source
//! (`ClosedItem::to_output_text` / `ReopenedItem::to_output_text` /
//! `actions/done.rs::emit_status_diagnostics`).

use pwf_application::pending_work::{done::CompletedPendingWork, reopen::ReopenedPendingWork};

use super::outcome::OutcomeRender;

/// The close confirmation: `"<Done|Cancelled> <id> (<project> :: <title>)\n"`,
/// followed by the `--review` task's `ADDED PWF TASK […]` block when present
/// (rendered through the single `OutcomeRender::raw_text` source, so the added
/// form never drifts from `add`'s).
pub(in crate::engines::pending_work) fn render_closed(outcome: &CompletedPendingWork) -> String {
    let mut text = format!(
        "{} {} ({} :: {})\n",
        outcome.action.past_tense(),
        outcome.id.as_ref(),
        outcome.project.as_ref(),
        outcome.title
    );
    if let Some(review) = &outcome.review_item {
        text.push_str(&review.raw_text("ADDED"));
    }
    text
}

/// The reopen confirmation: `"Reopened <id> (<project>)\n"`, or the idempotent
/// `"<id> already active (<project>) — skipped\n"` when nothing changed.
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

/// The done-queue side-effect diagnostics (stderr): futuro-header normalization
/// and section-cap eviction, in that order — verbatim from the former
/// `emit_status_diagnostics`.
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
