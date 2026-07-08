use cqrsy::Handler;
use pwf_domain::pending_work::AddedItem;

use crate::ports::{AddItemSpec, PendingWorkWriteStore};

#[derive(Debug, Clone, cqrsy::Command)]
#[command(out = pwf_domain::pending_work::AddedItem, err = AddPendingWorkError)]
pub struct AddPendingWorkItem {
    pub project_name: String,
    pub prompt: String,
    pub title: Option<String>,
    pub created: String,
    pub section: Option<String>,
    pub prereq: Option<String>,
    pub effort: Option<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum AddPendingWorkError {
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, Clone)]
pub struct AddPendingWorkItemHandler<S> {
    store: S,
}

impl<S> AddPendingWorkItemHandler<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }
}

impl<S> Handler<AddPendingWorkItem> for AddPendingWorkItemHandler<S>
where
    S: PendingWorkWriteStore,
{
    async fn handle(&self, req: AddPendingWorkItem) -> Result<AddedItem, AddPendingWorkError> {
        self.store
            .add_item(AddItemSpec {
                project_name: req.project_name,
                prompt: req.prompt,
                title: req.title,
                created: req.created,
                section: req.section,
                prereq: req.prereq,
                effort: req.effort,
            })
            .map_err(|error| AddPendingWorkError::WriteStore(Box::new(error)))
    }
}
