use crate::ports::{PendingWorkResolveStore, ResolvePendingWorkOutput};

#[derive(Debug, Clone)]
pub struct ResolvePendingWorkItem {
    pub id: String,
    pub show: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolvePendingWorkError {
    #[error("{0}")]
    ReadStore(Box<dyn std::error::Error + Send + Sync>),
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

#[cqrsy::handler(query)]
pub fn handle(
    store: &impl PendingWorkResolveStore,
    query: ResolvePendingWorkItem,
) -> Result<ResolvePendingWorkOutput, ResolvePendingWorkError> {
    resolve_from_store(store, &query.id, query.show)
}
