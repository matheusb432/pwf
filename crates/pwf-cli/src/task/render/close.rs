//! Renders byte-stable close and reopen confirmations plus ordered done-queue diagnostics.

use pwf_client::v1::{
    AddTaskResponse, CancelTaskResponse, ClosedTaskAction, CompleteTaskResponse, ReopenedTask,
    ReopenedTaskOutcome,
};

use super::confirmation::render_review_task;

struct ClosedTaskView<'a> {
    id: &'a str,
    project: &'a str,
    title: &'a str,
    action: i32,
    evicted_ids: &'a [String],
    futuro_renamed_project: Option<&'a str>,
    review_task: Option<&'a AddTaskResponse>,
}

pub(in crate::task) fn render_cancelled(outcome: &CancelTaskResponse) -> String {
    render_closed(&outcome.into())
}

pub(in crate::task) fn render_completed(outcome: &CompleteTaskResponse) -> String {
    render_closed(&outcome.into())
}

fn render_closed(outcome: &ClosedTaskView<'_>) -> String {
    let mut text = format!(
        "{} {} ({} :: {})\n",
        closed_action(outcome.action),
        outcome.id,
        outcome.project,
        outcome.title
    );
    if let Some(review) = outcome.review_task {
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

pub(in crate::task) fn emit_cancel_diagnostics(outcome: &CancelTaskResponse) {
    emit_close_diagnostics(&outcome.into());
}

pub(in crate::task) fn emit_complete_diagnostics(outcome: &CompleteTaskResponse) {
    emit_close_diagnostics(&outcome.into());
}

fn emit_close_diagnostics(outcome: &ClosedTaskView<'_>) {
    if let Some(project) = outcome.futuro_renamed_project {
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

impl<'a> From<&'a CancelTaskResponse> for ClosedTaskView<'a> {
    fn from(response: &'a CancelTaskResponse) -> Self {
        Self {
            id: &response.id,
            project: &response.project,
            title: &response.title,
            action: response.action,
            evicted_ids: &response.evicted_ids,
            futuro_renamed_project: response.futuro_renamed_project.as_deref(),
            review_task: response.review_task.as_ref(),
        }
    }
}

impl<'a> From<&'a CompleteTaskResponse> for ClosedTaskView<'a> {
    fn from(response: &'a CompleteTaskResponse) -> Self {
        Self {
            id: &response.id,
            project: &response.project,
            title: &response.title,
            action: response.action,
            evicted_ids: &response.evicted_ids,
            futuro_renamed_project: response.futuro_renamed_project.as_deref(),
            review_task: response.review_task.as_ref(),
        }
    }
}
