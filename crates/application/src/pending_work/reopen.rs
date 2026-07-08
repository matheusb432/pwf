use cqrsy::Handler;

use crate::ports::PendingWorkWriteStore;

#[derive(Debug, Clone, cqrsy::Command)]
#[command(out = String, err = ReopenPendingWorkError)]
pub struct ReopenPendingWork {
    pub id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ReopenPendingWorkError {
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, Clone)]
pub struct ReopenPendingWorkHandler<S> {
    store: S,
}

impl<S> ReopenPendingWorkHandler<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }
}

impl<S> Handler<ReopenPendingWork> for ReopenPendingWorkHandler<S>
where
    S: PendingWorkWriteStore,
{
    async fn handle(&self, req: ReopenPendingWork) -> Result<String, ReopenPendingWorkError> {
        self.store
            .reopen_item(&req.id)
            .map(|item| item.to_output_text())
            .map_err(|error| ReopenPendingWorkError::WriteStore(Box::new(error)))
    }
}
