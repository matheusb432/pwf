use std::time::Duration;

use anyhow::Context as _;
use futures::future::BoxFuture;
use pwf_application::ports::confirmation::{ConfirmationClient, ConfirmationClientError};
use tokio::sync::mpsc;
use tonic::{Code, Status, Streaming};

const CONFIRMATION_TIMEOUT: Duration = Duration::from_mins(30);

pub(super) struct GrpcConfirmationClient<Payload, ClientMessage, ServerMessage> {
    inbound: Streaming<ClientMessage>,
    outbound: mpsc::Sender<Result<ServerMessage, Status>>,
    preflight: fn(&Payload) -> ServerMessage,
    decision: fn(ClientMessage) -> Result<bool, ConfirmationClientError>,
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
        }
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
            (self.decision)(message)
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
