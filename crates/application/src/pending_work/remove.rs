use cqrsy::Handler;
use pwf_domain::pending_work::RemovedItem;

use crate::ports::PendingWorkWriteStore;

#[derive(Debug, Clone, cqrsy::Command)]
#[command(out = pwf_domain::pending_work::RemovedItem, err = RemovePendingWorkError)]
pub struct RemovePendingWorkItem {
    pub id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RemovePendingWorkError {
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, Clone)]
pub struct RemovePendingWorkItemHandler<S> {
    store: S,
}

impl<S> RemovePendingWorkItemHandler<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }
}

impl<S> Handler<RemovePendingWorkItem> for RemovePendingWorkItemHandler<S>
where
    S: PendingWorkWriteStore,
{
    async fn handle(
        &self,
        req: RemovePendingWorkItem,
    ) -> Result<RemovedItem, RemovePendingWorkError> {
        self.store
            .remove_item(&req.id)
            .map_err(|error| RemovePendingWorkError::WriteStore(Box::new(error)))
    }
}
