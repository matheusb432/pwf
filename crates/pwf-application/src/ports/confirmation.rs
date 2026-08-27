use futures::future::BoxFuture;

#[derive(Debug, thiserror::Error)]
pub enum ConfirmationClientError {
    #[error("confirmation interaction closed")]
    InteractionClosed,
    #[error("confirmation decision timed out")]
    DecisionTimedOut,
    #[error("confirmation client returned an unexpected message")]
    UnexpectedMessage,
    #[error(transparent)]
    ReceiveDecision(#[from] anyhow::Error),
}

pub trait ConfirmationClient: Send + Sync + 'static {
    type Confirmation: Sync;

    fn confirm<'a>(
        &'a mut self,
        confirmation: &'a Self::Confirmation,
    ) -> BoxFuture<'a, Result<bool, ConfirmationClientError>>;
}
