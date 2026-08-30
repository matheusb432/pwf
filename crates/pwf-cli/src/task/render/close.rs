//! Renders byte-stable close and reopen confirmations.

use pwf_client::pb::{ReopenTaskResult, reopen_task_result};

use super::confirmation::render_review_task;

pub(in crate::task) fn render_cancelled(task_id: &str, review_task_id: Option<&str>) -> String {
    render_closed("Cancelled", task_id, review_task_id)
}

pub(in crate::task) fn render_completed(task_id: &str, review_task_id: Option<&str>) -> String {
    render_closed("Done", task_id, review_task_id)
}

fn render_closed(action: &str, task_id: &str, review_task_id: Option<&str>) -> String {
    let mut text = format!("{action} {task_id}\n");
    if let Some(review_task_id) = review_task_id {
        text.push_str(&render_review_task(review_task_id));
    }
    text
}

/// Renders the byte-stable reopen or already-active confirmation.
pub(in crate::task) fn render_reopened(task_id: &str, outcome: ReopenTaskResult) -> String {
    match outcome.outcome {
        Some(reopen_task_result::Outcome::AlreadyActive(_)) => {
            format!("{task_id} is already active, so it was skipped.\n")
        }
        Some(reopen_task_result::Outcome::Reopened(_)) => format!("Reopened {task_id}\n"),
        Some(reopen_task_result::Outcome::Aborted(_)) => {
            format!("# reopen {task_id}: aborted\nnothing changed.\n")
        }
        None => format!("# reopen {task_id}: invalid server response\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_confirmation_includes_only_identifiers() {
        assert_eq!(render_completed("FOO-0001", None), "Done FOO-0001\n");
        assert_eq!(
            render_cancelled("FOO-0001", Some("FOO-0002")),
            "Cancelled FOO-0001\nADDED PWF TASK [FOO-0002]\n"
        );
    }
}
