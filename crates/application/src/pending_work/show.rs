use cqrsy::Handler;

use crate::{
    pending_work::resolve::{ResolvePendingWorkError, resolve_from_store},
    ports::{PendingWorkResolveStore, ResolvePendingWorkOutput},
};

pub type ShowPendingWorkError = ResolvePendingWorkError;

#[derive(Debug, Clone, cqrsy::Query)]
#[query(out = ResolvePendingWorkOutput, err = ShowPendingWorkError)]
pub struct ShowPendingWorkItem {
    pub id: String,
}

#[derive(Debug, Clone)]
pub struct ShowPendingWorkHandler<S> {
    store: S,
}

impl<S> ShowPendingWorkHandler<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }
}

impl<S> Handler<ShowPendingWorkItem> for ShowPendingWorkHandler<S>
where
    S: PendingWorkResolveStore,
{
    async fn handle(
        &self,
        req: ShowPendingWorkItem,
    ) -> Result<ResolvePendingWorkOutput, ShowPendingWorkError> {
        resolve_from_store(&self.store, &req.id, true)
    }
}
