use std::pin::Pin;

use futures::Stream;
use pwf_application::{
    ports::{confirmation::ConfirmationClientError, task_vault::TaskMutationError},
    task::{
        CloseTaskError, TaskMarkerSectionsError,
        activate_task::{self, ActivateTaskError},
        add_task::{self, AddTaskError},
        backlog_task::{self, BacklogTaskError},
        cancel_task::{self, CancelTaskError},
        clone_task::{self, CloneTaskError},
        complete_task::{self, CompleteTaskError},
        edit_task::{self, EditTaskError},
        get_task::{self, GetTaskError},
        get_task_dag::{self, GetTaskDagError},
        get_task_record::{self, GetTaskRecordError},
        list_tasks::{self, ListTasksError},
        remove_task::{self, RemoveTaskError},
        resolve_task_project::ResolveTaskProjectError,
    },
};
use pwf_wire::{
    confirmation::{ActivateTaskConfirmation, RemoveTaskConfirmation},
    pb, proto,
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
        let state = self.state.clone();
        let mutation_guard = state.task_mutations.clone().lock_owned().await;
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            let _mutation_guard = mutation_guard;
            runtime.block_on(add_task::execute(
                command,
                &state.store,
                &state.projects,
                &state.clock,
                &state.task_marker_sections,
            ))
        })
        .await
        .map_err(|error| Status::internal(format!("task creation worker failed: {error}")))?
        .map(proto::task::create_task_response)
        .map(Response::new)
        .map_err(create_task_status)
    }

    async fn clone_task(
        &self,
        request: Request<pb::CloneTaskRequest>,
    ) -> Result<Response<pb::CloneTaskResponse>, Status> {
        let command = request.into_inner().try_into()?;
        let _mutation_guard = self.state.task_mutations.lock().await;
        clone_task::execute(
            command,
            &self.state.store,
            &self.state.projects,
            &self.state.clock,
            &self.state.task_marker_sections,
        )
        .await
        .map(Into::into)
        .map(Response::new)
        .map_err(|error| match error {
            CloneTaskError::GetTask(error) => get_task_status(&error),
            CloneTaskError::AddTask(error) => create_task_status(error),
        })
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
            &self.state.projects,
            &self.state.clock,
        )
        .await
        .map(proto::task::cancel_task_response)
        .map(Response::new)
        .map_err(cancel_task_status)
    }

    async fn backlog_task(
        &self,
        request: Request<pb::BacklogTaskRequest>,
    ) -> Result<Response<pb::BacklogTaskResponse>, Status> {
        let command = request.into_inner().try_into()?;
        let _mutation_guard = self.state.task_mutations.lock().await;
        backlog_task::execute(&command, &self.state.store, &self.state.projects)
            .await
            .map(Into::into)
            .map(Response::new)
            .map_err(backlog_task_status)
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
            &self.state.projects,
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
        edit_task::execute(
            command,
            &self.state.store,
            &self.state.projects,
            &self.state.task_marker_sections,
        )
        .await
        .map(proto::task::update_task_response)
        .map(Response::new)
        .map_err(|error| edit_task_status(&error))
    }

    async fn get_task(
        &self,
        request: Request<pb::GetTaskRequest>,
    ) -> Result<Response<pb::GetTaskResponse>, Status> {
        let query = request.into_inner().try_into()?;
        get_task::execute(&query, &self.state.store, &self.state.projects)
            .await
            .map(Into::into)
            .map(Response::new)
            .map_err(|error| get_task_status(&error))
    }

    async fn get_task_record(
        &self,
        request: Request<pb::GetTaskRecordRequest>,
    ) -> Result<Response<pb::GetTaskRecordResponse>, Status> {
        let id = request.into_inner().try_into()?;
        get_task_record::execute(&id, &self.state.store, &self.state.projects)
            .await
            .map(Into::into)
            .map(Response::new)
            .map_err(|error| get_task_record_status(&error))
    }

    async fn get_task_dag(
        &self,
        request: Request<pb::GetTaskDagRequest>,
    ) -> Result<Response<pb::GetTaskDagResponse>, Status> {
        let query = proto::task::get_task_dag_request(request.into_inner())?;
        let graph = get_task_dag::execute(&query, &self.state.store, &self.state.projects)
            .await
            .map_err(|error| get_task_dag_status(&error))?;
        Ok(Response::new(proto::task::get_task_dag_response(graph)))
    }

    async fn list_tasks(
        &self,
        request: Request<pb::ListTasksRequest>,
    ) -> Result<Response<pb::ListTasksResponse>, Status> {
        let query = proto::task::list_tasks_request(request.into_inner())?;
        list_tasks::execute(
            &query,
            &self.state.store,
            &self.state.projects,
            &self.state.store,
            &self.state.task_list_snapshots,
            &self.state.user_settings,
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
            let item =
                remove_task::execute(&command, &state.store, &state.projects, &mut confirmation)
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

    type ActivateTaskStream = ResponseStream<pb::ActivateTaskResponse>;

    async fn activate_task(
        &self,
        request: Request<Streaming<pb::ActivateTaskRequest>>,
    ) -> Result<Response<Self::ActivateTaskStream>, Status> {
        let mut inbound = request.into_inner();
        let start = next_activate_start(&mut inbound).await?;
        let command = proto::task::activate_task_start(start)?;
        let (outbound, receiver) = mpsc::channel(STREAM_BUFFER);
        let mut confirmation = GrpcConfirmationClient::new(
            inbound,
            outbound.clone(),
            |confirmation: &ActivateTaskConfirmation| pb::ActivateTaskResponse {
                value: Some(pb::activate_task_response::Value::Preflight(
                    proto::task::activate_task_preflight(confirmation),
                )),
            },
            |message| match message.value {
                Some(pb::activate_task_request::Value::Decision(decision)) => {
                    Ok(decision.confirmed)
                }
                Some(pb::activate_task_request::Value::Start(_)) | None => {
                    Err(ConfirmationClientError::UnexpectedMessage)
                }
            },
        )
        .with_mutation_lock(self.state.task_mutations.clone());
        let state = self.state.clone();
        tokio::spawn(async move {
            confirmation.lock_mutations().await;
            let item =
                activate_task::execute(&command, &state.store, &state.projects, &mut confirmation)
                    .await
                    .map(proto::task::activate_task_result)
                    .map(|result| pb::ActivateTaskResponse {
                        value: Some(pb::activate_task_response::Value::Result(result)),
                    })
                    .map_err(activate_task_status);
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

async fn next_activate_start(
    inbound: &mut Streaming<pb::ActivateTaskRequest>,
) -> Result<pb::ActivateTaskStart, Status> {
    let message = inbound
        .message()
        .await?
        .ok_or_else(|| Status::invalid_argument("activate stream requires a start message"))?;
    match message.value {
        Some(pb::activate_task_request::Value::Start(start)) => Ok(start),
        Some(pb::activate_task_request::Value::Decision(_)) | None => Err(
            Status::invalid_argument("activate stream must start with start"),
        ),
    }
}

fn create_task_status(error: AddTaskError) -> Status {
    let message = error.to_string();
    match error {
        AddTaskError::GetProject(error) => get_project_status(&error),
        AddTaskError::UnknownBlockedByIds { .. }
        | AddTaskError::SelfBlockedBy { .. }
        | AddTaskError::BlockedByCycle { .. } => Status::failed_precondition(message),
        AddTaskError::InvalidTitle(_) => Status::invalid_argument(message),
        AddTaskError::MarkerSections(error) => marker_sections_status(&error),
        AddTaskError::MalformedBlockedBy { .. } => Status::data_loss(message),

        AddTaskError::WriteStore { .. }
        | AddTaskError::ReadBlockedBy { .. }
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
    }
}

fn cancel_task_status(error: CancelTaskError) -> Status {
    match error {
        CancelTaskError::ResolveProject(error) => resolve_task_project_status(&error),
        CancelTaskError::Close(error) => close_task_status(error),
        CancelTaskError::Clock(_) => Status::internal(error.to_string()),
    }
}

fn complete_task_status(error: CompleteTaskError) -> Status {
    match error {
        CompleteTaskError::ResolveProject(error) => resolve_task_project_status(&error),
        CompleteTaskError::Close(error) => close_task_status(error),
        CompleteTaskError::Clock(_) => Status::internal(error.to_string()),
    }
}

fn edit_task_status(error: &EditTaskError) -> Status {
    let message = error.to_string();
    match error {
        EditTaskError::TaskNotFound { .. } => Status::not_found(message),
        EditTaskError::Revision(_) => Status::aborted(message),
        EditTaskError::Mutation(error) => task_mutation_status(error),

        EditTaskError::InvalidTitle(_) => Status::invalid_argument(message),
        EditTaskError::MarkerSections(error) => marker_sections_status(error),
        EditTaskError::ClosedTask { .. }
        | EditTaskError::InvalidPersistedTitle { .. }
        | EditTaskError::UnknownBlockedByIds { .. }
        | EditTaskError::SelfBlockedBy { .. }
        | EditTaskError::BlockedByCycle { .. }
        | EditTaskError::AmbiguousMarkerSections { .. } => Status::failed_precondition(message),
        EditTaskError::InvalidTagsFrontmatter { .. } | EditTaskError::MalformedBlockedBy { .. } => {
            Status::data_loss(message)
        }
        EditTaskError::ReadBlockedBy { .. }
        | EditTaskError::WriteStore(_)
        | EditTaskError::QueryProject(_) => Status::internal(message),
    }
}

fn marker_sections_status(error: &TaskMarkerSectionsError) -> Status {
    match error {
        TaskMarkerSectionsError::Database(_) => Status::internal(error.to_string()),
        TaskMarkerSectionsError::InvalidSectionSet { .. }
        | TaskMarkerSectionsError::InvalidSection { .. }
        | TaskMarkerSectionsError::InvalidConfiguration(_)
        | TaskMarkerSectionsError::InvalidDefinitionCount { .. } => {
            Status::data_loss(error.to_string())
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
        GetTaskError::Read(error) => get_task_record_status(error),
        GetTaskError::Parse(_) => Status::data_loss(error.to_string()),
    }
}

fn get_task_record_status(error: &GetTaskRecordError) -> Status {
    match error {
        GetTaskRecordError::TaskNotFound { .. } => Status::not_found(error.to_string()),
        GetTaskRecordError::ReadStore(_) | GetTaskRecordError::QueryProject(_) => {
            Status::internal(error.to_string())
        }
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
        GetTaskDagError::QueryProject(_)
        | GetTaskDagError::ReadRoot { .. }
        | GetTaskDagError::ListProjectTasks { .. }
        | GetTaskDagError::InvalidGraph(_) => Status::internal(message),
    }
}

fn list_tasks_status(error: ListTasksError) -> Status {
    match error {
        ListTasksError::GetProject(error) => get_project_status(&error),
        ListTasksError::Settings(
            pwf_application::ports::user_settings::UserSettingsLoadError::InvalidConfiguration(
                error,
            ),
        ) => Status::failed_precondition(error.to_string()),
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
        RemoveTaskError::Revision(_) | RemoveTaskError::DeletionChanged => Status::aborted(message),
        RemoveTaskError::Mutation(error) => task_mutation_status(&error),

        RemoveTaskError::HasDependents { .. } => Status::failed_precondition(message),
        RemoveTaskError::InvalidTitle { .. } | RemoveTaskError::MalformedBlockedBy { .. } => {
            Status::data_loss(message)
        }
        RemoveTaskError::ReadDependents(_) | RemoveTaskError::WriteStore(_) => {
            Status::internal(message)
        }
    }
}

fn backlog_task_status(error: BacklogTaskError) -> Status {
    match error {
        BacklogTaskError::ClosedTask { .. } => Status::failed_precondition(error.to_string()),
        BacklogTaskError::Read(error) => get_task_record_status(&error),
        BacklogTaskError::Mutation(error) => task_mutation_status(&error),
    }
}

fn activate_task_status(error: ActivateTaskError) -> Status {
    match error {
        ActivateTaskError::Read(error) => get_task_record_status(&error),
        ActivateTaskError::Confirmation(error) => confirmation_status(&error),
        ActivateTaskError::Mutation(error) => task_mutation_status(&error),
    }
}
