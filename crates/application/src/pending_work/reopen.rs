use crate::ports::PendingWorkWriteStore;

#[derive(Debug, Clone)]
pub struct ReopenPendingWork {
    pub id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ReopenPendingWorkError {
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
}

#[cqrsy::handler(command)]
pub fn handle(
    store: &impl PendingWorkWriteStore,
    cmd: ReopenPendingWork,
) -> Result<String, ReopenPendingWorkError> {
    store
        .reopen_item(&cmd.id)
        .map(|item| item.to_output_text())
        .map_err(|error| ReopenPendingWorkError::WriteStore(Box::new(error)))
}
