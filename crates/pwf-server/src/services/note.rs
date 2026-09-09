use std::pin::Pin;

use futures::Stream;
use pwf_application::{
    note::{
        add_note::{self, AddNoteError},
        edit_note::{self, EditNoteError},
        list_notes::{self, ListNotesError},
        remove_note::{self, RemoveNoteError},
    },
    ports::confirmation::ConfirmationClientError,
};
use pwf_wire::{
    confirmation::RemoveNoteConfirmation,
    pb::{self, note_service_server::NoteService},
    proto,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};

use super::{
    confirmation::{GrpcConfirmationClient, confirmation_status},
    project::get_project_status,
};
use crate::AppState;

const STREAM_BUFFER: usize = 4;

type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

pub(crate) struct NoteGrpcService {
    state: AppState,
}

impl NoteGrpcService {
    pub(crate) fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl NoteService for NoteGrpcService {
    async fn add_note(
        &self,
        request: Request<pb::AddNoteRequest>,
    ) -> Result<Response<pb::AddNoteResponse>, Status> {
        let command = proto::note::add_note_request(request.into_inner())?;
        add_note::execute(
            command,
            &self.state.store,
            &self.state.pool,
            &self.state.clock,
        )
        .await
        .map(|note| proto::note::add_note_response(&note))
        .map(Response::new)
        .map_err(add_note_status)
    }

    async fn list_notes(
        &self,
        request: Request<pb::ListNotesRequest>,
    ) -> Result<Response<pb::ListNotesResponse>, Status> {
        let query = proto::note::list_notes_request(request.into_inner())?;
        list_notes::execute(query, &self.state.store, &self.state.pool)
            .await
            .map(proto::note::list_notes_response)
            .map(Response::new)
            .map_err(list_notes_status)
    }

    async fn update_note(
        &self,
        request: Request<pb::UpdateNoteRequest>,
    ) -> Result<Response<pb::UpdateNoteResponse>, Status> {
        let command = proto::note::update_note_request(request.into_inner())?;
        edit_note::execute(command, &self.state.store, &self.state.pool)
            .await
            .map(|note| proto::note::update_note_response(&note))
            .map(Response::new)
            .map_err(edit_note_status)
    }

    type DeleteNoteStream = ResponseStream<pb::DeleteNoteResponse>;

    async fn delete_note(
        &self,
        request: Request<Streaming<pb::DeleteNoteRequest>>,
    ) -> Result<Response<Self::DeleteNoteStream>, Status> {
        let mut inbound = request.into_inner();
        let start = next_delete_note_start(&mut inbound).await?;
        let command = proto::note::delete_note_start(&start)?;
        let (outbound, receiver) = mpsc::channel(STREAM_BUFFER);
        let mut confirmation = GrpcConfirmationClient::new(
            inbound,
            outbound.clone(),
            |confirmation: &RemoveNoteConfirmation| pb::DeleteNoteResponse {
                value: Some(pb::delete_note_response::Value::Preflight(
                    proto::note::delete_note_confirmation(confirmation),
                )),
            },
            |message| match message.value {
                Some(pb::delete_note_request::Value::Decision(decision)) => Ok(decision.confirmed),
                Some(pb::delete_note_request::Value::Start(_)) | None => {
                    Err(ConfirmationClientError::UnexpectedMessage)
                }
            },
        );
        let state = self.state.clone();
        tokio::spawn(async move {
            let item = remove_note::execute(command, &state.store, &state.pool, &mut confirmation)
                .await
                .map(proto::note::delete_note_result)
                .map(|result| pb::DeleteNoteResponse {
                    value: Some(pb::delete_note_response::Value::Result(result)),
                })
                .map_err(remove_note_status);
            let _ = outbound.send(item).await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }
}

async fn next_delete_note_start(
    inbound: &mut Streaming<pb::DeleteNoteRequest>,
) -> Result<pb::DeleteNoteStart, Status> {
    let message = inbound
        .message()
        .await?
        .ok_or_else(|| Status::invalid_argument("note delete stream requires a start message"))?;
    match message.value {
        Some(pb::delete_note_request::Value::Start(start)) => Ok(start),
        Some(pb::delete_note_request::Value::Decision(_)) | None => Err(Status::invalid_argument(
            "note delete stream must start with start",
        )),
    }
}

fn add_note_status(error: AddNoteError) -> Status {
    match error {
        AddNoteError::GetProject(error) => get_project_status(&error),
        AddNoteError::IdentifierExhausted { .. } => Status::resource_exhausted(error.to_string()),
        AddNoteError::Store(_) | AddNoteError::Clock(_) => Status::internal(error.to_string()),
    }
}

fn list_notes_status(error: ListNotesError) -> Status {
    match error {
        ListNotesError::GetProject(error) => get_project_status(&error),
        ListNotesError::Store(_) => Status::internal(error.to_string()),
    }
}

fn remove_note_status(error: RemoveNoteError) -> Status {
    match error {
        RemoveNoteError::GetProject(error) => get_project_status(&error),
        RemoveNoteError::ProjectMismatch { .. } => Status::failed_precondition(error.to_string()),
        RemoveNoteError::NoSuchNote { .. } => Status::not_found(error.to_string()),
        RemoveNoteError::Confirmation(error) => confirmation_status(&error),
        RemoveNoteError::Store(_) => Status::internal(error.to_string()),
    }
}

fn edit_note_status(error: EditNoteError) -> Status {
    match error {
        EditNoteError::GetProject(error) => get_project_status(&error),
        EditNoteError::ProjectMismatch { .. } => Status::failed_precondition(error.to_string()),
        EditNoteError::NoSuchNote { .. } => Status::not_found(error.to_string()),
        EditNoteError::Store(_) => Status::internal(error.to_string()),
    }
}
