use cqrsy::{Handle, Handler, Sender};
use pwf_domain::pending_work::AddedItem;

use crate::{
    AddPendingWorkItem,
    ports::{CompleteItemSpec, PendingWorkWriteStore, StatusTransitionOutput},
};

#[derive(Debug, Clone, cqrsy::Command)]
#[command(out = crate::ports::StatusTransitionOutput, err = CompletePendingWorkError)]
pub struct CompletePendingWork {
    pub id: String,
    pub completed: String,
    pub report: Option<String>,
    pub commits: Vec<String>,
    pub review: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CompletePendingWorkError {
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    ReviewTask(Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, Clone)]
pub struct CompletePendingWorkHandler<S, A> {
    store: S,
    add_sender: A,
}

impl<S, A> CompletePendingWorkHandler<S, A> {
    pub fn new(store: S, add_sender: A) -> Self {
        Self { store, add_sender }
    }
}

impl<S, A> Handler<CompletePendingWork> for CompletePendingWorkHandler<S, A>
where
    S: PendingWorkWriteStore,
    A: Sender<AddPendingWorkItem> + Handle,
{
    async fn handle(
        &self,
        req: CompletePendingWork,
    ) -> Result<StatusTransitionOutput, CompletePendingWorkError> {
        let commits = frontmatter_value(&req.commits);
        let closed = self
            .store
            .complete_item(CompleteItemSpec {
                id: req.id.clone(),
                completed: req.completed.clone(),
                report: req.report,
                commits: commits.clone(),
            })
            .map_err(|error| CompletePendingWorkError::WriteStore(Box::new(error)))?;
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
                .map_err(|error| CompletePendingWorkError::ReviewTask(Box::new(error)))?;
            text.push_str(&added_item_raw_text(&review));
        }
        Ok(StatusTransitionOutput {
            text,
            diagnostics: closed.diagnostics,
        })
    }
}

pub(super) fn frontmatter_value(values: &[String]) -> Option<String> {
    let mut ranges: Vec<String> = Vec::new();
    for value in values {
        for raw in value.split(',') {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            if !ranges.iter().any(|range| range == raw) {
                ranges.push(raw.to_string());
            }
        }
    }
    (!ranges.is_empty()).then(|| ranges.join(", "))
}

pub(super) fn review_task_prompt(reviewed_id: &str, range: Option<&str>) -> String {
    let diff = match range {
        Some(range) => format!("git-tools diff {range}"),
        None => "git-tools diff".to_string(),
    };
    format!("review {reviewed_id} & {diff} & git-tools diff-subrepos")
}

pub(super) fn added_item_raw_text(item: &AddedItem) -> String {
    format!(
        "ADDED PWF TASK [{}] {} :: {}\n  file: {}\n",
        item.id,
        item.project,
        item.title,
        item.note_path.display()
    )
}
