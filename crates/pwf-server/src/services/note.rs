use pwf_application::note::{
    add_note::{self, AddNoteError},
    list_notes::{self, ListNotesError},
    remove_note::{self, RemoveNoteError},
    update_note::{self, UpdateNoteError},
};
use pwf_wire::{
    proto,
    v1::{self, note_service_server::NoteService},
};
use tonic::{Request, Response, Status};

use super::project::resolve_project_status;
use crate::AppState;

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
        request: Request<v1::AddNoteRequest>,
    ) -> Result<Response<v1::AddNoteResponse>, Status> {
        let command = proto::note::add_note_request(request.into_inner())?;
        add_note::execute(
            command,
            &self.state.store,
            &self.state.pool,
            &self.state.clock,
        )
        .await
        .map(proto::note::add_note_response)
        .map(Response::new)
        .map_err(add_note_status)
    }

    async fn list_notes(
        &self,
        request: Request<v1::ListNotesRequest>,
    ) -> Result<Response<v1::ListNotesResponse>, Status> {
        let query = proto::note::list_notes_request(request.into_inner())?;
        list_notes::execute(query, &self.state.store, &self.state.pool)
            .await
            .map(proto::note::list_notes_response)
            .map(Response::new)
            .map_err(list_notes_status)
    }

    async fn remove_note(
        &self,
        request: Request<v1::RemoveNoteRequest>,
    ) -> Result<Response<v1::RemoveNoteResponse>, Status> {
        let command = proto::note::remove_note_request(request.into_inner())?;
        remove_note::execute(command, &self.state.store, &self.state.pool)
            .await
            .map(|id| proto::note::remove_note_response(&id))
            .map(Response::new)
            .map_err(remove_note_status)
    }

    async fn update_note(
        &self,
        request: Request<v1::UpdateNoteRequest>,
    ) -> Result<Response<v1::UpdateNoteResponse>, Status> {
        let command = proto::note::update_note_request(request.into_inner())?;
        update_note::execute(command, &self.state.store, &self.state.pool)
            .await
            .map(proto::note::update_note_response)
            .map(Response::new)
            .map_err(update_note_status)
    }
}

fn add_note_status(error: AddNoteError) -> Status {
    match error {
        AddNoteError::ResolveProject(error) => resolve_project_status(&error),
        AddNoteError::IdentifierExhausted { .. } => Status::resource_exhausted(error.to_string()),
        AddNoteError::Store(_) | AddNoteError::Clock(_) => Status::internal(error.to_string()),
    }
}

fn list_notes_status(error: ListNotesError) -> Status {
    match error {
        ListNotesError::ResolveProject(error) => resolve_project_status(&error),
        ListNotesError::Store(_) => Status::internal(error.to_string()),
    }
}

fn remove_note_status(error: RemoveNoteError) -> Status {
    match error {
        RemoveNoteError::ResolveProject(error) => resolve_project_status(&error),
        RemoveNoteError::ProjectMismatch { .. } => Status::failed_precondition(error.to_string()),
        RemoveNoteError::NoSuchNote { .. } => Status::not_found(error.to_string()),
        RemoveNoteError::Store(_) => Status::internal(error.to_string()),
    }
}

fn update_note_status(error: UpdateNoteError) -> Status {
    match error {
        UpdateNoteError::ResolveProject(error) => resolve_project_status(&error),
        UpdateNoteError::ProjectMismatch { .. } => Status::failed_precondition(error.to_string()),
        UpdateNoteError::NoSuchNote { .. } => Status::not_found(error.to_string()),
        UpdateNoteError::Store(_) => Status::internal(error.to_string()),
    }
}
