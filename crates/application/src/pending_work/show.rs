use crate::{
    pending_work::resolve::{ResolvePendingWorkError, resolve_from_store},
    ports::{PendingWorkResolveStore, ResolvePendingWorkOutput},
};

pub type ShowPendingWorkError = ResolvePendingWorkError;

#[derive(Debug, Clone)]
pub struct ShowPendingWorkItem {
    pub id: String,
}

#[cqrsy::handler(query)]
pub fn handle(
    store: &impl PendingWorkResolveStore,
    query: ShowPendingWorkItem,
) -> Result<ResolvePendingWorkOutput, ShowPendingWorkError> {
    resolve_from_store(store, &query.id, true)
}
