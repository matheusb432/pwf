use cqrsy::Handler;

use crate::ports::{PendingWorkResolveStore, ResolvePendingWorkOutput};

#[derive(Debug, Clone, cqrsy::Query)]
#[query(out = ResolvePendingWorkOutput, err = ResolvePendingWorkError)]
pub struct ResolvePendingWorkItem {
    pub id: String,
    pub show: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolvePendingWorkError {
    #[error("{0}")]
    ReadStore(Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, Clone)]
pub struct ResolvePendingWorkHandler<S> {
    store: S,
}

impl<S> ResolvePendingWorkHandler<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }
}

pub(crate) fn resolve_from_store<S>(
    store: &S,
    id: &str,
    show: bool,
) -> Result<ResolvePendingWorkOutput, ResolvePendingWorkError>
where
    S: PendingWorkResolveStore,
{
    store
        .resolve_item(id, show)
        .map_err(|error| ResolvePendingWorkError::ReadStore(Box::new(error)))
}

impl<S> Handler<ResolvePendingWorkItem> for ResolvePendingWorkHandler<S>
where
    S: PendingWorkResolveStore,
{
    async fn handle(
        &self,
        req: ResolvePendingWorkItem,
    ) -> Result<ResolvePendingWorkOutput, ResolvePendingWorkError> {
        resolve_from_store(&self.store, &req.id, req.show)
    }
}
