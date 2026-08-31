use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use futures::future::BoxFuture;
use pwf_application::ports::confirmation::{ConfirmationClient, ConfirmationClientError};
use tokio::sync::{Mutex, OwnedMutexGuard, mpsc};
use tonic::{Code, Status, Streaming};

const CONFIRMATION_TIMEOUT: Duration = Duration::from_mins(30);

pub(super) struct GrpcConfirmationClient<Payload, ClientMessage, ServerMessage> {
    inbound: Streaming<ClientMessage>,
    outbound: mpsc::Sender<Result<ServerMessage, Status>>,
    preflight: fn(&Payload) -> ServerMessage,
    decision: fn(ClientMessage) -> Result<bool, ConfirmationClientError>,
    mutation_lock: Option<Arc<Mutex<()>>>,
    mutation_guard: Option<OwnedMutexGuard<()>>,
}

impl<Payload, ClientMessage, ServerMessage>
    GrpcConfirmationClient<Payload, ClientMessage, ServerMessage>
{
    pub(super) fn new(
        inbound: Streaming<ClientMessage>,
        outbound: mpsc::Sender<Result<ServerMessage, Status>>,
        preflight: fn(&Payload) -> ServerMessage,
        decision: fn(ClientMessage) -> Result<bool, ConfirmationClientError>,
    ) -> Self {
        Self {
            inbound,
            outbound,
            preflight,
            decision,
            mutation_lock: None,
            mutation_guard: None,
        }
    }

    pub(super) fn with_mutation_lock(mut self, mutation_lock: Arc<Mutex<()>>) -> Self {
        self.mutation_lock = Some(mutation_lock);
        self
    }

    async fn lock_mutations_after_confirmation(&mut self, confirmed: bool) {
        if !confirmed {
            return;
        }
        let Some(mutation_lock) = self.mutation_lock.clone() else {
            return;
        };
        self.mutation_guard = Some(mutation_lock.lock_owned().await);
    }
}

impl<Payload, ClientMessage, ServerMessage> ConfirmationClient
    for GrpcConfirmationClient<Payload, ClientMessage, ServerMessage>
where
    Payload: Send + Sync + 'static,
    ClientMessage: Send + Sync + 'static,
    ServerMessage: Send + 'static,
{
    type Confirmation = Payload;

    fn confirm<'a>(
        &'a mut self,
        confirmation: &'a Payload,
    ) -> BoxFuture<'a, Result<bool, ConfirmationClientError>> {
        Box::pin(async move {
            let preflight = (self.preflight)(confirmation);
            self.outbound
                .send(Ok(preflight))
                .await
                .map_err(|_| ConfirmationClientError::InteractionClosed)?;
            let message = tokio::time::timeout(CONFIRMATION_TIMEOUT, self.inbound.message())
                .await
                .map_err(|_| ConfirmationClientError::DecisionTimedOut)?
                .context("failed to receive confirmation decision")?
                .ok_or(ConfirmationClientError::InteractionClosed)?;
            let confirmed = (self.decision)(message)?;
            self.lock_mutations_after_confirmation(confirmed).await;
            Ok(confirmed)
        })
    }
}

pub(super) fn confirmation_status(error: &ConfirmationClientError) -> Status {
    let code = match error {
        ConfirmationClientError::InteractionClosed => Code::Cancelled,
        ConfirmationClientError::DecisionTimedOut => Code::DeadlineExceeded,
        ConfirmationClientError::UnexpectedMessage => Code::InvalidArgument,
        ConfirmationClientError::ReceiveDecision(_) => Code::Unavailable,
    };
    Status::new(code, error.to_string())
}
