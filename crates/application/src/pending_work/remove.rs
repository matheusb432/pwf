use pwf_domain::pending_work::RemovedItem;

use crate::ports::PendingWorkWriteStore;

#[derive(Debug, Clone)]
pub struct RemovePendingWorkItem {
    pub id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RemovePendingWorkError {
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
}

#[cqrsy::handler(command)]
pub fn handle(
    store: &impl PendingWorkWriteStore,
    cmd: RemovePendingWorkItem,
) -> Result<RemovedItem, RemovePendingWorkError> {
    store
        .remove_item(&cmd.id)
        .map_err(|error| RemovePendingWorkError::WriteStore(Box::new(error)))
}
