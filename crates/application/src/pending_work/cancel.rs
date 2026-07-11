use cqrsy::Sender;

use crate::{
    AddPendingWorkItem,
    pending_work::done::{added_item_raw_text, frontmatter_value, review_task_prompt},
    ports::{CancelItemSpec, PendingWorkWriteStore, StatusTransitionOutput},
};

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
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    ReviewTask(Box<dyn std::error::Error + Send + Sync>),
}

#[cqrsy::handler(command)]
pub async fn handle(
    store: &impl PendingWorkWriteStore,
    add_sender: &impl Sender<AddPendingWorkItem>,
    cmd: CancelPendingWork,
) -> Result<StatusTransitionOutput, CancelPendingWorkError> {
    let commits = frontmatter_value(&cmd.commits);
    let closed = store
        .cancel_item(CancelItemSpec {
            id: cmd.id.clone(),
            completed: cmd.completed.clone(),
            report: cmd.report,
            commits: commits.clone(),
        })
        .map_err(|error| CancelPendingWorkError::WriteStore(Box::new(error)))?;
    let mut text = closed.to_output_text();
    if cmd.review {
        let review = add_sender
            .send(AddPendingWorkItem {
                project_name: closed.project.clone(),
                prompt: review_task_prompt(&closed.id, commits.as_deref()),
                title: None,
                created: cmd.completed,
                section: Some("Human".to_string()),
                prereq: None,
                effort: None,
                tags: None,
            })
            .await
            .map_err(|error| CancelPendingWorkError::ReviewTask(Box::new(error)))?;
        text.push_str(&added_item_raw_text(&review));
    }
    Ok(StatusTransitionOutput {
        text,
        diagnostics: closed.diagnostics,
    })
}
