use pwf_wire::v1::{self, note_service_client::NoteServiceClient};

use crate::{AuthenticatedChannel, ClientError, RequestPolicy};

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
        request: v1::AddNoteRequest,
    ) -> Result<v1::AddedNote, ClientError> {
        self.client()
            .add_note(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn list_notes(
        &self,
        request: v1::ListNotesRequest,
    ) -> Result<v1::ListedNotes, ClientError> {
        self.client()
            .list_notes(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn remove_note(
        &self,
        request: v1::RemoveNoteRequest,
    ) -> Result<v1::RemovedNote, ClientError> {
        self.client()
            .remove_note(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn update_note(
        &self,
        request: v1::UpdateNoteRequest,
    ) -> Result<v1::UpdatedNote, ClientError> {
        self.client()
            .update_note(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    fn client(&self) -> NoteServiceClient<AuthenticatedChannel> {
        NoteServiceClient::with_interceptor(self.channel.clone(), self.request_policy.clone())
            .max_encoding_message_size(super::MAX_REQUEST_MESSAGE_SIZE)
            .max_decoding_message_size(super::MAX_RESPONSE_MESSAGE_SIZE)
    }
}
