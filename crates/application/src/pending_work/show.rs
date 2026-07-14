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
#[expect(
    clippy::needless_pass_by_value,
    reason = "the cqrsy show operation owns its request by contract"
)]
pub fn execute(
    query: ShowPendingWorkItem,
    store: &impl PendingWorkResolveStore,
) -> Result<ResolvePendingWorkOutput, ShowPendingWorkError> {
    resolve_from_store(store, &query.id, true)
}
