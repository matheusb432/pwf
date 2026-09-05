use pwf_wire::pb::{self, note_service_client::NoteServiceClient};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    ClientError, PolicyChannel, RequestPolicy,
    confirmation::{Confirmation, ConfirmationPrompt, ConfirmedRequestError, protocol},
};

const STREAM_BUFFER: usize = 2;

#[derive(Clone)]
pub struct NoteClient {
    channel: tonic::transport::Channel,
    request_policy: RequestPolicy,
}

impl NoteClient {
    pub(crate) fn new(channel: tonic::transport::Channel, request_policy: RequestPolicy) -> Self {
        Self {
            channel,
            request_policy,
        }
    }

    pub async fn add_note(
        &self,
        request: pb::AddNoteRequest,
    ) -> Result<pb::AddNoteResponse, ClientError> {
        self.client()
            .add_note(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn list_notes(
        &self,
        request: pb::ListNotesRequest,
    ) -> Result<pb::ListNotesResponse, ClientError> {
        self.client()
            .list_notes(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn delete_note<Prompt>(
        &self,
        request: pb::DeleteNoteStart,
        prompt: Prompt,
    ) -> Result<pb::DeleteNoteResult, ConfirmedRequestError<Prompt::Error>>
    where
        Prompt: ConfirmationPrompt,
    {
        let (sender, receiver) = mpsc::channel(STREAM_BUFFER);
        sender
            .send(pb::DeleteNoteRequest {
                value: Some(pb::delete_note_request::Value::Start(request)),
            })
            .await
            .map_err(|_| protocol("note delete request stream closed"))?;
        let mut stream = self
            .client()
            .delete_note(ReceiverStream::new(receiver))
            .await
            .map_err(ConfirmedRequestError::Operation)?
            .into_inner();
        let first = stream
            .message()
            .await
            .map_err(ConfirmedRequestError::Operation)?
            .ok_or_else(|| protocol("note delete response stream closed before preflight"))?;
        match first.value {
            Some(pb::delete_note_response::Value::Preflight(preflight)) => {
                let confirmed = prompt
                    .confirm(&Confirmation::DeleteNote(preflight))
                    .map_err(ConfirmedRequestError::Prompt)?;
                sender
                    .send(pb::DeleteNoteRequest {
                        value: Some(pb::delete_note_request::Value::Decision(
                            pb::ConfirmationDecision { confirmed },
                        )),
                    })
                    .await
                    .map_err(|_| protocol("note delete decision stream closed"))?;
                let result = stream
                    .message()
                    .await
                    .map_err(ConfirmedRequestError::Operation)?
                    .ok_or_else(|| protocol("note delete response stream closed before result"))?;
                match result.value {
                    Some(pb::delete_note_response::Value::Result(result)) => Ok(result),
                    Some(pb::delete_note_response::Value::Preflight(_)) | None => Err(protocol(
                        "note delete response stream returned an invalid result",
                    )),
                }
            }
            Some(pb::delete_note_response::Value::Result(result)) => Ok(result),
            None => Err(protocol(
                "note delete response stream returned an empty message",
            )),
        }
    }

    pub async fn update_note(
        &self,
        request: pb::UpdateNoteRequest,
    ) -> Result<pb::UpdateNoteResponse, ClientError> {
        self.client()
            .update_note(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    fn client(&self) -> NoteServiceClient<PolicyChannel> {
        NoteServiceClient::with_interceptor(self.channel.clone(), self.request_policy)
            .max_encoding_message_size(super::MAX_REQUEST_MESSAGE_SIZE)
            .max_decoding_message_size(super::MAX_RESPONSE_MESSAGE_SIZE)
    }
}
