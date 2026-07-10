use cqrsy::{Handle, Handler, Sender};

use crate::{
    AddPendingWorkItem,
    pending_work::done::{added_item_raw_text, frontmatter_value, review_task_prompt},
    ports::{CancelItemSpec, PendingWorkWriteStore, StatusTransitionOutput},
};

#[derive(Debug, Clone, cqrsy::Command)]
#[command(out = crate::ports::StatusTransitionOutput, err = CancelPendingWorkError)]
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

#[derive(Debug, Clone)]
pub struct CancelPendingWorkHandler<S, A> {
    store: S,
    add_sender: A,
}

impl<S, A> CancelPendingWorkHandler<S, A> {
    pub fn new(store: S, add_sender: A) -> Self {
        Self { store, add_sender }
    }
}

impl<S, A> Handler<CancelPendingWork> for CancelPendingWorkHandler<S, A>
where
    S: PendingWorkWriteStore,
    A: Sender<AddPendingWorkItem> + Handle,
{
    async fn handle(
        &self,
        req: CancelPendingWork,
    ) -> Result<StatusTransitionOutput, CancelPendingWorkError> {
        let commits = frontmatter_value(&req.commits);
        let closed = self
            .store
            .cancel_item(CancelItemSpec {
                id: req.id.clone(),
                completed: req.completed.clone(),
                report: req.report,
                commits: commits.clone(),
            })
            .map_err(|error| CancelPendingWorkError::WriteStore(Box::new(error)))?;
        let mut text = closed.to_output_text();
        if req.review {
            let review = self
                .add_sender
                .send(AddPendingWorkItem {
                    project_name: closed.project.clone(),
                    prompt: review_task_prompt(&closed.id, commits.as_deref()),
                    title: None,
                    created: req.completed,
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
}
