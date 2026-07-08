use cqrsy::Handler;
use pwf_domain::pending_work::UpdatedItem;

use crate::ports::{PendingWorkWriteStore, UpdateItemSpec};

#[derive(Debug, Clone, cqrsy::Command)]
#[command(out = pwf_domain::pending_work::UpdatedItem, err = UpdatePendingWorkError)]
pub struct UpdatePendingWorkItem {
    pub id: String,
    pub prompt: Option<String>,
    pub title: Option<String>,
    pub append: Option<String>,
    pub prereq: Vec<String>,
    pub clear_prereq: bool,
    pub commits: Option<String>,
    pub append_report: Option<String>,
    pub effort: Option<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdatePendingWorkError {
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, Clone)]
pub struct UpdatePendingWorkItemHandler<S> {
    store: S,
}

impl<S> UpdatePendingWorkItemHandler<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }
}

impl<S> Handler<UpdatePendingWorkItem> for UpdatePendingWorkItemHandler<S>
where
    S: PendingWorkWriteStore,
{
    async fn handle(
        &self,
        req: UpdatePendingWorkItem,
    ) -> Result<UpdatedItem, UpdatePendingWorkError> {
        self.store
            .update_item(UpdateItemSpec {
                id: req.id,
                prompt: req.prompt,
                title: req.title,
                append: req.append,
                prereq: req.prereq,
                clear_prereq: req.clear_prereq,
                commits: req.commits,
                append_report: req.append_report,
                effort: req.effort,
            })
            .map_err(|error| UpdatePendingWorkError::WriteStore(Box::new(error)))
    }
}
