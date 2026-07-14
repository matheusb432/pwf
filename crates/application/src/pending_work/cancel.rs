use super::{
    add::{AddPendingWorkError, AddPendingWorkItem},
    done::{added_item_raw_text, frontmatter_value, review_task_prompt},
};
use crate::ports::{CancelItemSpec, PendingWorkWriteStore, StatusTransitionOutput};

#[derive(Debug, Clone)]
pub struct CancelPendingWork {
    pub id: String,
    pub completed: String,
    report: String,
    pub commits: Vec<String>,
    pub review: bool,
}

impl CancelPendingWork {
    pub fn new(
        id: String,
        completed: String,
        report: String,
        commits: Vec<String>,
        review: bool,
    ) -> Result<Self, CancelPendingWorkError> {
        if report.trim().is_empty() {
            return Err(CancelPendingWorkError::EmptyReport);
        }
        Ok(Self {
            id,
            completed,
            report,
            commits,
            review,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CancelPendingWorkError {
    #[error("--report cannot be empty.")]
    EmptyReport,
    #[error("{0}")]
    // TODO refactor to not be boxed once infra/ coupled business logic is moved to application/
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    ReviewTask(#[source] AddPendingWorkError),
}

#[cqrsy::handler(command)]
pub fn execute(
    command: CancelPendingWork,
    store: &impl PendingWorkWriteStore,
) -> Result<StatusTransitionOutput, CancelPendingWorkError> {
    let commits = frontmatter_value(&command.commits);
    let closed = store
        .cancel_item(CancelItemSpec {
            id: command.id.clone(),
            completed: command.completed.clone(),
            report: command.report,
            commits: commits.clone(),
        })
        .map_err(|error| CancelPendingWorkError::WriteStore(Box::new(error)))?;
    let mut text = closed.to_output_text();
    let review_item = if command.review {
        let review = super::add::execute(
            AddPendingWorkItem {
                project_name: closed.project.clone(),
                prompt: review_task_prompt(&closed.id, commits.as_deref()),
                title: None,
                created: command.completed,
                section: Some("Human".to_string()),
                prereq: None,
                effort: None,
                tags: None,
            },
            store,
        )
        .map_err(CancelPendingWorkError::ReviewTask)?;
        text.push_str(&added_item_raw_text(&review));
        Some(review)
    } else {
        None
    };
    Ok(StatusTransitionOutput {
        text,
        diagnostics: closed.diagnostics,
        review_item,
    })
}

#[cfg(test)]
mod tests {
    use super::{CancelPendingWork, CancelPendingWorkError};
    use crate::pending_work::add::AddPendingWorkError;

    #[test]
    fn blank_report_is_rejected_before_execution() {
        let error = CancelPendingWork::new(
            "PWF-0128".to_string(),
            "2026-07-14".to_string(),
            " \t\n".to_string(),
            Vec::new(),
            false,
        )
        .expect_err("blank cancellation report must fail");

        assert!(matches!(error, CancelPendingWorkError::EmptyReport));
    }

    #[test]
    fn review_task_error_retains_the_concrete_add_error() {
        let error = CancelPendingWorkError::ReviewTask(AddPendingWorkError::WriteStore(Box::new(
            std::io::Error::other("index write failed"),
        )));

        let CancelPendingWorkError::ReviewTask(source) = error else {
            panic!("expected review-task error");
        };
        assert!(matches!(source, AddPendingWorkError::WriteStore(_)));
    }
}
