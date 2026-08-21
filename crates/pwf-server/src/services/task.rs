use std::{pin::Pin, sync::Arc, time::Duration};

use futures::{Stream, future::BoxFuture};
use prost::Message as _;
use pwf_application::{
    contract::confirmation::Confirmation,
    ports::confirmation::ConfirmationClient,
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
use pwf_wire::v1::{self, task_service_server::TaskService};
use tokio::sync::{Mutex, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Code, Request, Response, Status, Streaming};

use super::project::resolve_project_status;
use crate::{
    AppState,
    conversion::{input, output},
};

const CONFIRMATION_TIMEOUT: Duration = Duration::from_mins(30);
const STREAM_BUFFER: usize = 4;

type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[derive(Clone)]
pub(crate) struct TaskApi {
    state: AppState,
}

impl TaskApi {
    pub(crate) fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl TaskService for TaskApi {
    async fn add_task(
        &self,
        request: Request<v1::AddTaskRequest>,
    ) -> Result<Response<v1::AddedTask>, Status> {
        let command = input::add_task(request.into_inner())?;
        add_task::execute(
            &command,
            &self.state.store,
            &self.state.pool,
            &self.state.clock,
        )
        .await
        .map(output::added_task)
        .map(Response::new)
        .map_err(add_task_status)
    }

    async fn cancel_task(
        &self,
        request: Request<v1::CancelTaskRequest>,
    ) -> Result<Response<v1::ClosedTask>, Status> {
        let command = input::cancel_task(request.into_inner())?;
        cancel_task::execute(
            &command,
            &self.state.store,
            &self.state.pool,
            &self.state.clock,
        )
        .await
        .map(output::closed_task)
        .map(Response::new)
        .map_err(cancel_task_status)
    }

    async fn complete_task(
        &self,
        request: Request<v1::CompleteTaskRequest>,
    ) -> Result<Response<v1::ClosedTask>, Status> {
        let command = input::complete_task(request.into_inner())?;
        complete_task::execute(
            &command,
            &self.state.store,
            &self.state.pool,
            &self.state.clock,
        )
        .await
        .map(output::closed_task)
        .map(Response::new)
        .map_err(complete_task_status)
    }

    async fn edit_task(
        &self,
        request: Request<v1::EditTaskRequest>,
    ) -> Result<Response<v1::EditedTask>, Status> {
        let command = input::edit_task(request.into_inner())?;
        edit_task::execute(command, &self.state.store, &self.state.pool)
            .await
            .map(|task| output::edited_task(&task))
            .map(Response::new)
            .map_err(|error| edit_task_status(&error))
    }

    async fn get_task(
        &self,
        request: Request<v1::GetTaskRequest>,
    ) -> Result<Response<v1::TaskRead>, Status> {
        let query = input::get_task(request.into_inner())?;
        get_task::execute(&query, &self.state.store, &self.state.pool)
            .await
            .map(output::task_read)
            .map(Response::new)
            .map_err(|error| get_task_status(&error))
    }

    async fn list_tasks(
        &self,
        request: Request<v1::ListTasksRequest>,
    ) -> Result<Response<v1::ListedTasks>, Status> {
        let query = input::list_tasks(request.into_inner())?;
        list_tasks::execute(
            &query,
            &self.state.store,
            &self.state.pool,
            &self.state.store,
        )
        .await
        .map(output::listed_tasks)
        .map(Response::new)
        .map_err(list_tasks_status)
    }

    type RemoveTaskStream = ResponseStream<v1::RemoveTaskServerMessage>;

    async fn remove_task(
        &self,
        request: Request<Streaming<v1::RemoveTaskClientMessage>>,
    ) -> Result<Response<Self::RemoveTaskStream>, Status> {
        let mut inbound = request.into_inner();
        let start = next_remove_start(&mut inbound).await?;
        let command = input::remove_task(start)?;
        let (outbound, receiver) = mpsc::channel(STREAM_BUFFER);
        let confirmation = Arc::new(RemoveConfirmation::new(inbound, outbound.clone()));
        let state = self.state.clone();
        tokio::spawn(async move {
            let result =
                remove_task::execute(&command, &state.store, &state.pool, confirmation.as_ref())
                    .await;
            let item = match confirmation.take_failure() {
                Some(status) => Err(status),
                None => result
                    .map(output::removed_task_outcome)
                    .map(|result| v1::RemoveTaskServerMessage {
                        value: Some(v1::remove_task_server_message::Value::Result(result)),
                    })
                    .map_err(remove_task_status),
            };
            let _ = outbound.send(item).await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }

    type ReopenTaskStream = ResponseStream<v1::ReopenTaskServerMessage>;

    async fn reopen_task(
        &self,
        request: Request<Streaming<v1::ReopenTaskClientMessage>>,
    ) -> Result<Response<Self::ReopenTaskStream>, Status> {
        let mut inbound = request.into_inner();
        let start = next_reopen_start(&mut inbound).await?;
        let command = input::reopen_task(start)?;
        let (outbound, receiver) = mpsc::channel(STREAM_BUFFER);
        let confirmation = Arc::new(ReopenConfirmation::new(inbound, outbound.clone()));
        let state = self.state.clone();
        tokio::spawn(async move {
            let result =
                reopen_task::execute(&command, &state.store, &state.pool, confirmation.as_ref())
                    .await;
            let item = match confirmation.take_failure() {
                Some(status) => Err(status),
                None => result
                    .map(output::reopened_task)
                    .map(|result| v1::ReopenTaskServerMessage {
                        value: Some(v1::reopen_task_server_message::Value::Result(result)),
                    })
                    .map_err(reopen_task_status),
            };
            let _ = outbound.send(item).await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }
}

async fn next_remove_start(
    inbound: &mut Streaming<v1::RemoveTaskClientMessage>,
) -> Result<v1::RemoveTaskRequest, Status> {
    let message = inbound
        .message()
        .await?
        .ok_or_else(|| Status::invalid_argument("remove stream requires a start message"))?;
    match message.value {
        Some(v1::remove_task_client_message::Value::Start(start)) => Ok(start),
        Some(v1::remove_task_client_message::Value::Decision(_)) | None => Err(
            Status::invalid_argument("remove stream must start with start"),
        ),
    }
}

async fn next_reopen_start(
    inbound: &mut Streaming<v1::ReopenTaskClientMessage>,
) -> Result<v1::ReopenTaskRequest, Status> {
    let message = inbound
        .message()
        .await?
        .ok_or_else(|| Status::invalid_argument("reopen stream requires a start message"))?;
    match message.value {
        Some(v1::reopen_task_client_message::Value::Start(start)) => Ok(start),
        Some(v1::reopen_task_client_message::Value::Decision(_)) | None => Err(
            Status::invalid_argument("reopen stream must start with start"),
        ),
    }
}

struct RemoveConfirmation {
    inbound: Mutex<Streaming<v1::RemoveTaskClientMessage>>,
    outbound: mpsc::Sender<Result<v1::RemoveTaskServerMessage, Status>>,
    failure: std::sync::Mutex<Option<Status>>,
}

impl RemoveConfirmation {
    fn new(
        inbound: Streaming<v1::RemoveTaskClientMessage>,
        outbound: mpsc::Sender<Result<v1::RemoveTaskServerMessage, Status>>,
    ) -> Self {
        Self {
            inbound: Mutex::new(inbound),
            outbound,
            failure: std::sync::Mutex::new(None),
        }
    }

    fn fail(&self, status: Status) {
        if let Ok(mut failure) = self.failure.lock() {
            *failure = Some(status);
        }
    }

    fn take_failure(&self) -> Option<Status> {
        self.failure.lock().ok().and_then(|mut value| value.take())
    }
}

impl ConfirmationClient for RemoveConfirmation {
    fn confirm<'a>(&'a self, confirmation: &'a Confirmation) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let Confirmation::RemoveTask(confirmation) = confirmation else {
                self.fail(Status::internal(
                    "remove operation produced the wrong confirmation",
                ));
                return false;
            };
            let preflight = v1::RemoveTaskServerMessage {
                value: Some(v1::remove_task_server_message::Value::Preflight(
                    output::remove_confirmation(confirmation),
                )),
            };
            if self.outbound.send(Ok(preflight)).await.is_err() {
                self.fail(Status::cancelled("remove confirmation stream closed"));
                return false;
            }
            let mut inbound = self.inbound.lock().await;
            let decision = tokio::time::timeout(CONFIRMATION_TIMEOUT, inbound.message()).await;
            match decision {
                Ok(Ok(Some(message))) => match message.value {
                    Some(v1::remove_task_client_message::Value::Decision(decision)) => {
                        decision.confirmed
                    }
                    Some(v1::remove_task_client_message::Value::Start(_)) | None => {
                        self.fail(Status::invalid_argument(
                            "remove stream requires one decision after preflight",
                        ));
                        false
                    }
                },
                Ok(Ok(None)) => {
                    self.fail(Status::cancelled("remove confirmation stream closed"));
                    false
                }
                Ok(Err(status)) => {
                    self.fail(status);
                    false
                }
                Err(_) => {
                    self.fail(Status::deadline_exceeded("remove confirmation timed out"));
                    false
                }
            }
        })
    }
}

struct ReopenConfirmation {
    inbound: Mutex<Streaming<v1::ReopenTaskClientMessage>>,
    outbound: mpsc::Sender<Result<v1::ReopenTaskServerMessage, Status>>,
    failure: std::sync::Mutex<Option<Status>>,
}

impl ReopenConfirmation {
    fn new(
        inbound: Streaming<v1::ReopenTaskClientMessage>,
        outbound: mpsc::Sender<Result<v1::ReopenTaskServerMessage, Status>>,
    ) -> Self {
        Self {
            inbound: Mutex::new(inbound),
            outbound,
            failure: std::sync::Mutex::new(None),
        }
    }

    fn fail(&self, status: Status) {
        if let Ok(mut failure) = self.failure.lock() {
            *failure = Some(status);
        }
    }

    fn take_failure(&self) -> Option<Status> {
        self.failure.lock().ok().and_then(|mut value| value.take())
    }
}

impl ConfirmationClient for ReopenConfirmation {
    fn confirm<'a>(&'a self, confirmation: &'a Confirmation) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let Confirmation::ReopenTask(confirmation) = confirmation else {
                self.fail(Status::internal(
                    "reopen operation produced the wrong confirmation",
                ));
                return false;
            };
            let preflight = v1::ReopenTaskServerMessage {
                value: Some(v1::reopen_task_server_message::Value::Preflight(
                    output::reopen_confirmation(confirmation),
                )),
            };
            if self.outbound.send(Ok(preflight)).await.is_err() {
                self.fail(Status::cancelled("reopen confirmation stream closed"));
                return false;
            }
            let mut inbound = self.inbound.lock().await;
            let decision = tokio::time::timeout(CONFIRMATION_TIMEOUT, inbound.message()).await;
            match decision {
                Ok(Ok(Some(message))) => match message.value {
                    Some(v1::reopen_task_client_message::Value::Decision(decision)) => {
                        decision.confirmed
                    }
                    Some(v1::reopen_task_client_message::Value::Start(_)) | None => {
                        self.fail(Status::invalid_argument(
                            "reopen stream requires one decision after preflight",
                        ));
                        false
                    }
                },
                Ok(Ok(None)) => {
                    self.fail(Status::cancelled("reopen confirmation stream closed"));
                    false
                }
                Ok(Err(status)) => {
                    self.fail(status);
                    false
                }
                Err(_) => {
                    self.fail(Status::deadline_exceeded("reopen confirmation timed out"));
                    false
                }
            }
        })
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
    diagnostics: &pwf_application::contract::task::AddTaskDiagnostics,
) -> Status {
    let details = output::add_task_failure_details(diagnostics).encode_to_vec();
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
        ReopenTaskError::WriteStore(_) => Status::internal(error.to_string()),
    }
}
