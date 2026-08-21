use futures::future::BoxFuture;

use crate::contract::task::session::PreparedSessionDispatch;

/// Collects the frontend-owned decision for one prepared session dispatch.
pub trait SessionConfirmationClient {
    fn confirm<'a>(&'a self, prepared: &'a PreparedSessionDispatch) -> BoxFuture<'a, bool>;
}
