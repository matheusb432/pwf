use std::pin::Pin;

use futures::Stream;
use pwf_application::{
    ports::{confirmation::ConfirmationClientError, task_vault::TaskMutationError},
    task::{
        CloseTaskError, MutationRequestError, TaskPromptLanesError,
        add_task::{self, AddTaskError},
        cancel_task::{self, CancelTaskError},
        complete_task::{self, CompleteTaskError},
        edit_task::{self, EditTaskError},
        get_task::{self, GetTaskError},
        get_task_dag::{self, GetTaskDagError},
        list_tasks::{self, ListTasksError},
        remove_task::{self, RemoveTaskError},
        reopen_task::{self, ReopenTaskError},
        resolve_task_project::ResolveTaskProjectError,
    },
};
use pwf_wire::{
    confirmation::{RemoveTaskConfirmation, ReopenTaskConfirmation},
    pb, proto,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};

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
impl pb::task_service_server::TaskService for TaskGrpcService {
    async fn create_task(
        &self,
        request: Request<pb::CreateTaskRequest>,
    ) -> Result<Response<pb::CreateTaskResponse>, Status> {
        let command = proto::task::create_task_request(request.into_inner())?;
        let _mutation_guard = self.state.task_mutations.lock().await;
        add_task::execute(
            &command,
            &self.state.store,
            &self.state.pool,
            &self.state.clock,
        )
        .await
        .map(|id| proto::task::create_task_response(&id))
        .map(Response::new)
        .map_err(create_task_status)
    }

    async fn cancel_task(
        &self,
        request: Request<pb::CancelTaskRequest>,
    ) -> Result<Response<pb::CancelTaskResponse>, Status> {
        let command = proto::task::cancel_task_request(request.into_inner())?;
        let _mutation_guard = self.state.task_mutations.lock().await;
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
        request: Request<pb::CompleteTaskRequest>,
    ) -> Result<Response<pb::CompleteTaskResponse>, Status> {
        let command = proto::task::complete_task_request(request.into_inner())?;
        let _mutation_guard = self.state.task_mutations.lock().await;
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

    async fn update_task(
        &self,
        request: Request<pb::UpdateTaskRequest>,
    ) -> Result<Response<pb::UpdateTaskResponse>, Status> {
        let command = proto::task::update_task_request(request.into_inner())?;
        let _mutation_guard = self.state.task_mutations.lock().await;
        edit_task::execute(command, &self.state.store, &self.state.pool)
            .await
            .map(|()| proto::task::update_task_response())
            .map(Response::new)
            .map_err(|error| edit_task_status(&error))
    }

    async fn get_task(
        &self,
        request: Request<pb::GetTaskRequest>,
    ) -> Result<Response<pb::GetTaskResponse>, Status> {
        let query = proto::task::get_task_request(request.into_inner())?;
        get_task::execute(&query, &self.state.store, &self.state.pool)
            .await
            .map(proto::task::get_task_response)
            .map(Response::new)
            .map_err(|error| get_task_status(&error))
    }

    async fn get_task_dag(
        &self,
        request: Request<pb::GetTaskDagRequest>,
    ) -> Result<Response<pb::GetTaskDagResponse>, Status> {
        let query = proto::task::get_task_dag_request(request.into_inner())?;
        let graph = get_task_dag::execute(&query, &self.state.store, &self.state.pool)
            .await
            .map_err(|error| get_task_dag_status(&error))?;
        proto::task::get_task_dag_response(graph)
            .map(Response::new)
            .map_err(|error| Status::internal(error.to_string()))
    }

    async fn list_tasks(
        &self,
        request: Request<pb::ListTasksRequest>,
    ) -> Result<Response<pb::ListTasksResponse>, Status> {
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

    type DeleteTaskStream = ResponseStream<pb::DeleteTaskResponse>;

    async fn delete_task(
        &self,
        request: Request<Streaming<pb::DeleteTaskRequest>>,
    ) -> Result<Response<Self::DeleteTaskStream>, Status> {
        let mut inbound = request.into_inner();
        let start = next_delete_start(&mut inbound).await?;
        let command = proto::task::delete_task_start(start)?;
        let (outbound, receiver) = mpsc::channel(STREAM_BUFFER);
        let mut confirmation = GrpcConfirmationClient::new(
            inbound,
            outbound.clone(),
            |confirmation: &RemoveTaskConfirmation| pb::DeleteTaskResponse {
                value: Some(pb::delete_task_response::Value::Preflight(
                    proto::task::delete_task_preflight(confirmation),
                )),
            },
            |message| match message.value {
                Some(pb::delete_task_request::Value::Decision(decision)) => Ok(decision.confirmed),
                Some(pb::delete_task_request::Value::Start(_)) | None => {
                    Err(ConfirmationClientError::UnexpectedMessage)
                }
            },
        )
        .with_mutation_lock(self.state.task_mutations.clone());
        let state = self.state.clone();
        tokio::spawn(async move {
            let item = remove_task::execute(&command, &state.store, &state.pool, &mut confirmation)
                .await
                .map(proto::task::delete_task_result)
                .map(|result| pb::DeleteTaskResponse {
                    value: Some(pb::delete_task_response::Value::Result(result)),
                })
                .map_err(delete_task_status);
            let _ = outbound.send(item).await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }

    type ReopenTaskStream = ResponseStream<pb::ReopenTaskResponse>;

    async fn reopen_task(
        &self,
        request: Request<Streaming<pb::ReopenTaskRequest>>,
    ) -> Result<Response<Self::ReopenTaskStream>, Status> {
        let mut inbound = request.into_inner();
        let start = next_reopen_start(&mut inbound).await?;
        let command = proto::task::reopen_task_start(start)?;
        let (outbound, receiver) = mpsc::channel(STREAM_BUFFER);
        let mut confirmation = GrpcConfirmationClient::new(
            inbound,
            outbound.clone(),
            |confirmation: &ReopenTaskConfirmation| pb::ReopenTaskResponse {
                value: Some(pb::reopen_task_response::Value::Preflight(
                    proto::task::reopen_task_preflight(confirmation),
                )),
            },
            |message| match message.value {
                Some(pb::reopen_task_request::Value::Decision(decision)) => Ok(decision.confirmed),
                Some(pb::reopen_task_request::Value::Start(_)) | None => {
                    Err(ConfirmationClientError::UnexpectedMessage)
                }
            },
        )
        .with_mutation_lock(self.state.task_mutations.clone());
        let state = self.state.clone();
        tokio::spawn(async move {
            let item = reopen_task::execute(&command, &state.store, &state.pool, &mut confirmation)
                .await
                .map(proto::task::reopen_task_result)
                .map(|result| pb::ReopenTaskResponse {
                    value: Some(pb::reopen_task_response::Value::Result(result)),
                })
                .map_err(reopen_task_status);
            let _ = outbound.send(item).await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }
}

async fn next_delete_start(
    inbound: &mut Streaming<pb::DeleteTaskRequest>,
) -> Result<pb::DeleteTaskStart, Status> {
    let message = inbound
        .message()
        .await?
        .ok_or_else(|| Status::invalid_argument("delete stream requires a start message"))?;
    match message.value {
        Some(pb::delete_task_request::Value::Start(start)) => Ok(start),
        Some(pb::delete_task_request::Value::Decision(_)) | None => Err(Status::invalid_argument(
            "delete stream must start with start",
        )),
    }
}

async fn next_reopen_start(
    inbound: &mut Streaming<pb::ReopenTaskRequest>,
) -> Result<pb::ReopenTaskStart, Status> {
    let message = inbound
        .message()
        .await?
        .ok_or_else(|| Status::invalid_argument("reopen stream requires a start message"))?;
    match message.value {
        Some(pb::reopen_task_request::Value::Start(start)) => Ok(start),
        Some(pb::reopen_task_request::Value::Decision(_)) | None => Err(Status::invalid_argument(
            "reopen stream must start with start",
        )),
    }
}

fn create_task_status(error: AddTaskError) -> Status {
    let message = error.to_string();
    match error {
        AddTaskError::ProjectResolution(error) => resolve_project_status(&error),
        AddTaskError::UnknownBlockedByIds { .. }
        | AddTaskError::SelfBlockedBy { .. }
        | AddTaskError::BlockedByCycle { .. } => Status::failed_precondition(message),
        AddTaskError::InvalidTitle(_) => Status::invalid_argument(message),
        AddTaskError::PromptLanes(error) => prompt_lanes_status(&error),
        AddTaskError::MalformedBlockedBy { .. } | AddTaskError::ReservedTaskChanged { .. } => {
            Status::data_loss(message)
        }
        AddTaskError::MutationRequest(error) => mutation_request_status(&error),
        AddTaskError::WriteStore { .. }
        | AddTaskError::ReadBlockedBy { .. }
        | AddTaskError::ReadReservedTask { .. }
        | AddTaskError::QueryProject(_)
        | AddTaskError::AllocateTaskId { .. }
        | AddTaskError::Clock(_) => Status::internal(message),
    }
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
        CloseTaskError::Revision(_) => Status::aborted(message),
        CloseTaskError::Mutation(error) => task_mutation_status(&error),
        CloseTaskError::WriteStore(_) => Status::internal(message),
        CloseTaskError::ReviewTask(error) => create_task_status(*error),
    }
}

fn cancel_task_status(error: CancelTaskError) -> Status {
    match error {
        CancelTaskError::ResolveProject(error) => resolve_task_project_status(&error),
        CancelTaskError::Close(error) => close_task_status(error),
        CancelTaskError::Clock(_) => Status::internal(error.to_string()),
        CancelTaskError::MutationRequest(error) => mutation_request_status(&error),
        CancelTaskError::PromptLanes(error) => prompt_lanes_status(&error),
    }
}

fn complete_task_status(error: CompleteTaskError) -> Status {
    match error {
        CompleteTaskError::ResolveProject(error) => resolve_task_project_status(&error),
        CompleteTaskError::Close(error) => close_task_status(error),
        CompleteTaskError::Clock(_) => Status::internal(error.to_string()),
        CompleteTaskError::MutationRequest(error) => mutation_request_status(&error),
        CompleteTaskError::PromptLanes(error) => prompt_lanes_status(&error),
    }
}

fn edit_task_status(error: &EditTaskError) -> Status {
    let message = error.to_string();
    match error {
        EditTaskError::TaskNotFound { .. } => Status::not_found(message),
        EditTaskError::Revision(_) => Status::aborted(message),
        EditTaskError::Mutation(error) => task_mutation_status(error),
        EditTaskError::MutationRequest(error) => mutation_request_status(error),
        EditTaskError::InvalidTitle(_) => Status::invalid_argument(message),
        EditTaskError::PromptLanes(error) => prompt_lanes_status(error),
        EditTaskError::ClosedTask { .. }
        | EditTaskError::NoteMissing { .. }
        | EditTaskError::InvalidPersistedTitle { .. }
        | EditTaskError::UnknownBlockedByIds { .. }
        | EditTaskError::SelfBlockedBy { .. }
        | EditTaskError::BlockedByCycle { .. }
        | EditTaskError::AmbiguousLanes { .. } => Status::failed_precondition(message),
        EditTaskError::InvalidTagsFrontmatter { .. } | EditTaskError::MalformedBlockedBy { .. } => {
            Status::data_loss(message)
        }
        EditTaskError::ReadBlockedBy { .. }
        | EditTaskError::WriteStore(_)
        | EditTaskError::QueryProject(_) => Status::internal(message),
    }
}

fn prompt_lanes_status(error: &TaskPromptLanesError) -> Status {
    match error {
        TaskPromptLanesError::Database(_) => Status::internal(error.to_string()),
        TaskPromptLanesError::InvalidLaneSet { .. }
        | TaskPromptLanesError::InvalidLane { .. }
        | TaskPromptLanesError::InvalidConfiguration(_)
        | TaskPromptLanesError::InvalidDefinitionCount { .. } => {
            Status::data_loss(error.to_string())
        }
    }
}

fn mutation_request_status(error: &MutationRequestError) -> Status {
    match error {
        MutationRequestError::Conflict { .. } => Status::already_exists(error.to_string()),
        MutationRequestError::Incomplete { .. } => Status::aborted(error.to_string()),
        MutationRequestError::Corrupt { .. } => Status::data_loss(error.to_string()),
        MutationRequestError::InvalidIdentity { .. } | MutationRequestError::Database(_) => {
            Status::internal(error.to_string())
        }
    }
}

fn task_mutation_status<E: std::fmt::Display>(error: &TaskMutationError<E>) -> Status {
    match error {
        TaskMutationError::StaleTask { .. } | TaskMutationError::SourceChanged => {
            Status::aborted(error.to_string())
        }
        TaskMutationError::Store(_) => Status::internal(error.to_string()),
    }
}

fn get_task_status(error: &GetTaskError) -> Status {
    match error {
        GetTaskError::TaskNotFound { .. } => Status::not_found(error.to_string()),
        GetTaskError::InvalidTaskData { .. } | GetTaskError::MalformedBlockedBy { .. } => {
            Status::data_loss(error.to_string())
        }
        _ => Status::internal(error.to_string()),
    }
}

fn get_task_dag_status(error: &GetTaskDagError) -> Status {
    let message = error.to_string();
    match error {
        GetTaskDagError::UnknownProjectId { .. } | GetTaskDagError::TaskNotFound { .. } => {
            Status::not_found(message)
        }
        GetTaskDagError::Cycle { .. } => Status::data_loss(message),
        GetTaskDagError::NodeLimit { .. } | GetTaskDagError::EdgeLimit { .. } => {
            Status::resource_exhausted(message)
        }
        GetTaskDagError::ListProjects(_)
        | GetTaskDagError::ReadRoot { .. }
        | GetTaskDagError::ListProjectTasks { .. } => Status::internal(message),
    }
}

fn list_tasks_status(error: ListTasksError) -> Status {
    match error {
        ListTasksError::ResolveProject(error) => resolve_project_status(&error),
        ListTasksError::InvalidPageToken { .. } => Status::invalid_argument(error.to_string()),
        error => Status::internal(error.to_string()),
    }
}

fn delete_task_status(error: RemoveTaskError) -> Status {
    let message = error.to_string();
    match error {
        RemoveTaskError::TaskNotFound { .. } => Status::not_found(message),
        RemoveTaskError::ResolveProject(error) => resolve_task_project_status(&error),
        RemoveTaskError::Confirmation(error) => confirmation_status(&error),
        RemoveTaskError::Revision(_) => Status::aborted(message),
        RemoveTaskError::Mutation(error) => task_mutation_status(&error),
        RemoveTaskError::MutationRequest(error) => mutation_request_status(&error),
        RemoveTaskError::NoteMissing { .. } | RemoveTaskError::HasDependents { .. } => {
            Status::failed_precondition(message)
        }
        RemoveTaskError::InvalidTitle { .. } | RemoveTaskError::MalformedBlockedBy { .. } => {
            Status::data_loss(message)
        }
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
        ReopenTaskError::Revision(_) => Status::aborted(error.to_string()),
        ReopenTaskError::Mutation(error) => task_mutation_status(&error),
        ReopenTaskError::MutationRequest(error) => mutation_request_status(&error),
        ReopenTaskError::WriteStore(_) => Status::internal(error.to_string()),
    }
}
