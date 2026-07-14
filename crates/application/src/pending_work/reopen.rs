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
#[expect(
    clippy::needless_pass_by_value,
    reason = "the cqrsy reopen operation owns its request by contract"
)]
pub fn execute(
    cmd: ReopenPendingWork,
    store: &impl PendingWorkWriteStore,
) -> Result<String, ReopenPendingWorkError> {
    store
        .reopen_item(&cmd.id)
        .map(|item| item.to_output_text())
        .map_err(|error| ReopenPendingWorkError::WriteStore(Box::new(error)))
}
