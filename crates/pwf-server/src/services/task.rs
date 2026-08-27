use std::pin::Pin;

use futures::Stream;
use prost::Message as _;
use pwf_application::{
    ports::confirmation::ConfirmationClientError,
    task::{
        CloseTaskError,
        add_task::{self, AddTaskError},
        cancel_task::{self, CancelTaskError},
        complete_task::{self, CompleteTaskError},
        edit_task::{self, EditTaskError},
        get_task::{self, GetTaskError},
        list_tasks::{self, ListTasksError},
        remove_task::{self, RemoveTaskError},
        reopen_task::{self, ReopenTaskError},
        resolve_task_project::ResolveTaskProjectError,
    },
};
use pwf_wire::{
    confirmation::{RemoveTaskConfirmation, ReopenTaskConfirmation},
    proto,
    v1::{self, task_service_server::TaskService},
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Code, Request, Response, Status, Streaming};

use super::{
    confirmation::{GrpcConfirmationClient, confirmation_status},
    project::resolve_project_status,
};
use crate::AppState;

const STREAM_BUFFER: usize = 4;

type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

pub(crate) struct TaskGrpcService {
    state: AppState,
}

impl TaskGrpcService {
    pub(crate) fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl TaskService for TaskGrpcService {
    async fn add_task(
        &self,
        request: Request<v1::AddTaskRequest>,
    ) -> Result<Response<v1::AddTaskResponse>, Status> {
        let command = proto::task::add_task_request(request.into_inner())?;
        add_task::execute(
            &command,
            &self.state.store,
            &self.state.pool,
            &self.state.clock,
        )
        .await
        .map(proto::task::add_task_response)
        .map(Response::new)
        .map_err(add_task_status)
    }

    async fn cancel_task(
        &self,
        request: Request<v1::CancelTaskRequest>,
    ) -> Result<Response<v1::CancelTaskResponse>, Status> {
        let command = proto::task::cancel_task_request(request.into_inner())?;
        cancel_task::execute(
            &command,
            &self.state.store,
            &self.state.pool,
            &self.state.clock,
        )
        .await
        .map(proto::task::cancel_task_response)
        .map(Response::new)
        .map_err(cancel_task_status)
    }

    async fn complete_task(
        &self,
        request: Request<v1::CompleteTaskRequest>,
    ) -> Result<Response<v1::CompleteTaskResponse>, Status> {
        let command = proto::task::complete_task_request(request.into_inner())?;
        complete_task::execute(
            &command,
            &self.state.store,
            &self.state.pool,
            &self.state.clock,
        )
        .await
        .map(proto::task::complete_task_response)
        .map(Response::new)
        .map_err(complete_task_status)
    }

    async fn edit_task(
        &self,
        request: Request<v1::EditTaskRequest>,
    ) -> Result<Response<v1::EditTaskResponse>, Status> {
        let command = proto::task::edit_task_request(request.into_inner())?;
        edit_task::execute(command, &self.state.store, &self.state.pool)
            .await
            .map(proto::task::edit_task_response)
            .map(Response::new)
            .map_err(|error| edit_task_status(&error))
    }

    async fn get_task(
        &self,
        request: Request<v1::GetTaskRequest>,
    ) -> Result<Response<v1::GetTaskResponse>, Status> {
        let query = proto::task::get_task_request(request.into_inner())?;
        get_task::execute(&query, &self.state.store, &self.state.pool)
            .await
            .map(proto::task::get_task_response)
            .map(Response::new)
            .map_err(|error| get_task_status(&error))
    }

    async fn list_tasks(
        &self,
        request: Request<v1::ListTasksRequest>,
    ) -> Result<Response<v1::ListTasksResponse>, Status> {
        let query = proto::task::list_tasks_request(request.into_inner())?;
        list_tasks::execute(
            &query,
            &self.state.store,
            &self.state.pool,
            &self.state.store,
        )
        .await
        .map(proto::task::list_tasks_response)
        .map(Response::new)
        .map_err(list_tasks_status)
    }

    type RemoveTaskStream = ResponseStream<v1::RemoveTaskResponse>;

    async fn remove_task(
        &self,
        request: Request<Streaming<v1::RemoveTaskRequest>>,
    ) -> Result<Response<Self::RemoveTaskStream>, Status> {
        let mut inbound = request.into_inner();
        let start = next_remove_start(&mut inbound).await?;
        let task_id = proto::task::remove_task_start(start)?;
        let (outbound, receiver) = mpsc::channel(STREAM_BUFFER);
        let mut confirmation = GrpcConfirmationClient::new(
            inbound,
            outbound.clone(),
            |confirmation: &RemoveTaskConfirmation| v1::RemoveTaskResponse {
                value: Some(v1::remove_task_response::Value::Preflight(
                    proto::task::remove_task_confirmation(confirmation),
                )),
            },
            |message| match message.value {
                Some(v1::remove_task_request::Value::Decision(decision)) => Ok(decision.confirmed),
                Some(v1::remove_task_request::Value::Start(_)) | None => {
                    Err(ConfirmationClientError::UnexpectedMessage)
                }
            },
        );
        let state = self.state.clone();
        tokio::spawn(async move {
            let item = remove_task::execute(&task_id, &state.store, &state.pool, &mut confirmation)
                .await
                .map(proto::task::remove_task_result)
                .map(|result| v1::RemoveTaskResponse {
                    value: Some(v1::remove_task_response::Value::Result(result)),
                })
                .map_err(remove_task_status);
            let _ = outbound.send(item).await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }

    type ReopenTaskStream = ResponseStream<v1::ReopenTaskResponse>;

    async fn reopen_task(
        &self,
        request: Request<Streaming<v1::ReopenTaskRequest>>,
    ) -> Result<Response<Self::ReopenTaskStream>, Status> {
        let mut inbound = request.into_inner();
        let start = next_reopen_start(&mut inbound).await?;
        let task_id = proto::task::reopen_task_start(start)?;
        let (outbound, receiver) = mpsc::channel(STREAM_BUFFER);
        let mut confirmation = GrpcConfirmationClient::new(
            inbound,
            outbound.clone(),
            |confirmation: &ReopenTaskConfirmation| v1::ReopenTaskResponse {
                value: Some(v1::reopen_task_response::Value::Preflight(
                    proto::task::reopen_task_confirmation(confirmation),
                )),
            },
            |message| match message.value {
                Some(v1::reopen_task_request::Value::Decision(decision)) => Ok(decision.confirmed),
                Some(v1::reopen_task_request::Value::Start(_)) | None => {
                    Err(ConfirmationClientError::UnexpectedMessage)
                }
            },
        );
        let state = self.state.clone();
        tokio::spawn(async move {
            let item = reopen_task::execute(&task_id, &state.store, &state.pool, &mut confirmation)
                .await
                .map(proto::task::reopen_task_result)
                .map(|result| v1::ReopenTaskResponse {
                    value: Some(v1::reopen_task_response::Value::Result(result)),
                })
                .map_err(reopen_task_status);
            let _ = outbound.send(item).await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }
}

async fn next_remove_start(
    inbound: &mut Streaming<v1::RemoveTaskRequest>,
) -> Result<v1::RemoveTaskStart, Status> {
    let message = inbound
        .message()
        .await?
        .ok_or_else(|| Status::invalid_argument("remove stream requires a start message"))?;
    match message.value {
        Some(v1::remove_task_request::Value::Start(start)) => Ok(start),
        Some(v1::remove_task_request::Value::Decision(_)) | None => Err(Status::invalid_argument(
            "remove stream must start with start",
        )),
    }
}

async fn next_reopen_start(
    inbound: &mut Streaming<v1::ReopenTaskRequest>,
) -> Result<v1::ReopenTaskStart, Status> {
    let message = inbound
        .message()
        .await?
        .ok_or_else(|| Status::invalid_argument("reopen stream requires a start message"))?;
    match message.value {
        Some(v1::reopen_task_request::Value::Start(start)) => Ok(start),
        Some(v1::reopen_task_request::Value::Decision(_)) | None => Err(Status::invalid_argument(
            "reopen stream must start with start",
        )),
    }
}

fn add_task_status(error: AddTaskError) -> Status {
    let message = error.to_string();
    match error {
        AddTaskError::ProjectResolution(error) => resolve_project_status(&error),
        AddTaskError::UnknownBlockedByIds { .. }
        | AddTaskError::SelfBlockedBy { .. }
        | AddTaskError::BlockedByCycle { .. } => Status::failed_precondition(message),
        AddTaskError::InvalidTitle(_) => Status::invalid_argument(message),
        AddTaskError::ReadBlockedBy { .. } | AddTaskError::MalformedBlockedBy { .. } => {
            Status::failed_precondition(message)
        }
        AddTaskError::WriteStore { diagnostics, .. } => {
            status_with_add_details(message, &diagnostics)
        }
        AddTaskError::QueryProject(_) | AddTaskError::AllocateTaskId { .. } => {
            Status::internal(message)
        }
    }
}

fn status_with_add_details(
    message: String,
    diagnostics: &pwf_wire::task::AddTaskDiagnostics,
) -> Status {
    let details = proto::task::add_task_failure_details(diagnostics);
    let details = details.encode_to_vec();
    Status::with_details(Code::Internal, message, details.into())
}

fn resolve_task_project_status(error: &ResolveTaskProjectError) -> Status {
    match error {
        ResolveTaskProjectError::UnknownProjectId { .. } => Status::not_found(error.to_string()),
        ResolveTaskProjectError::QueryProject(_) => Status::internal(error.to_string()),
    }
}

fn close_task_status(error: CloseTaskError) -> Status {
    let message = error.to_string();
    match error {
        CloseTaskError::TaskNotFound { .. } | CloseTaskError::UnknownProjectId { .. } => {
            Status::not_found(message)
        }
        CloseTaskError::InvalidTitle { .. } => Status::failed_precondition(message),
        CloseTaskError::WriteStore(_) => Status::internal(message),
        CloseTaskError::ReviewTask(error) => add_task_status(*error),
    }
}

fn cancel_task_status(error: CancelTaskError) -> Status {
    match error {
        CancelTaskError::ResolveProject(error) => resolve_task_project_status(&error),
        CancelTaskError::Close(error) => close_task_status(error),
    }
}

fn complete_task_status(error: CompleteTaskError) -> Status {
    match error {
        CompleteTaskError::ResolveProject(error) => resolve_task_project_status(&error),
        CompleteTaskError::Close(error) => close_task_status(error),
    }
}

fn edit_task_status(error: &EditTaskError) -> Status {
    let message = error.to_string();
    match error {
        EditTaskError::TaskNotFound { .. } => Status::not_found(message),
        EditTaskError::ClosedTask { .. }
        | EditTaskError::InvalidPersistedTitle { .. }
        | EditTaskError::InvalidTagsFrontmatter { .. }
        | EditTaskError::MalformedBlockedBy { .. }
        | EditTaskError::UnknownBlockedByIds { .. }
        | EditTaskError::ReadBlockedBy { .. }
        | EditTaskError::SelfBlockedBy { .. }
        | EditTaskError::BlockedByCycle { .. }
        | EditTaskError::AmbiguousLanes { .. } => Status::failed_precondition(message),
        EditTaskError::WriteStore(_) | EditTaskError::QueryProject(_) => Status::internal(message),
    }
}

fn get_task_status(error: &GetTaskError) -> Status {
    if matches!(error, GetTaskError::TaskNotFound { .. }) {
        Status::not_found(error.to_string())
    } else {
        Status::internal(error.to_string())
    }
}

fn list_tasks_status(error: ListTasksError) -> Status {
    match error {
        ListTasksError::ResolveProject(error) => resolve_project_status(&error),
        error => Status::internal(error.to_string()),
    }
}

fn remove_task_status(error: RemoveTaskError) -> Status {
    let message = error.to_string();
    match error {
        RemoveTaskError::TaskNotFound { .. } => Status::not_found(message),
        RemoveTaskError::ResolveProject(error) => resolve_task_project_status(&error),
        RemoveTaskError::Confirmation(error) => confirmation_status(&error),
        RemoveTaskError::NoteMissing { .. }
        | RemoveTaskError::InvalidTitle { .. }
        | RemoveTaskError::HasDependents { .. }
        | RemoveTaskError::MalformedBlockedBy { .. } => Status::failed_precondition(message),
        RemoveTaskError::ReadDependents(_) | RemoveTaskError::WriteStore(_) => {
            Status::internal(message)
        }
    }
}

fn reopen_task_status(error: ReopenTaskError) -> Status {
    match error {
        ReopenTaskError::TaskNotFound { .. } => Status::not_found(error.to_string()),
        ReopenTaskError::ResolveProject(error) => resolve_task_project_status(&error),
        ReopenTaskError::Confirmation(error) => confirmation_status(&error),
        ReopenTaskError::WriteStore(_) => Status::internal(error.to_string()),
    }
}
