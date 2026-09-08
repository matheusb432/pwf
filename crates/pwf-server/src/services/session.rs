use std::pin::Pin;

use futures::Stream;
use pwf_application::{
    ports::{confirmation::ConfirmationClientError, task_vault::TaskMutationError},
    task::session::{
        dispatch_confirmed_session::{self, DispatchConfirmedSessionError},
        dispatch_session::DispatchSessionError,
        plan_session::{self, PlanSessionError, SessionPlanningClients},
    },
};
use pwf_infra::session::{AgentHarness, ProcessEnvironment};
use pwf_wire::{
    pb, proto,
    task::session::{PlannedSession, PreparedSessionDispatch},
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};

use super::confirmation::{GrpcConfirmationClient, confirmation_status};
use crate::AppState;

const STREAM_BUFFER: usize = 4;
const ENVIRONMENT_VARIABLES_MAX: usize = 512;
const ENVIRONMENT_KEY_BYTES_MAX: usize = 256;
const ENVIRONMENT_VALUE_BYTES_MAX: usize = 32 * 1024;

type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

pub(crate) struct SessionGrpcService {
    state: AppState,
}

impl SessionGrpcService {
    pub(crate) fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl pb::session_service_server::SessionService for SessionGrpcService {
    async fn plan_session(
        &self,
        request: Request<pb::PlanSessionRequest>,
    ) -> Result<Response<pb::PlanSessionResponse>, Status> {
        let mut request = request.into_inner();
        let environment = process_environment(&mut request)?;
        let command = proto::session::plan_session_request(request)?;
        let agent = AgentHarness::new(environment.clone());
        let clients = SessionPlanningClients::new(agent, self.state.project_directory);
        let planned = plan_session::execute(
            &command,
            &self.state.store,
            &self.state.pool,
            &self.state.home,
            &clients,
        )
        .await
        .map_err(|error| plan_session_status(&error))?;
        match planned {
            PlannedSession::DryRun(value) => {
                Ok(Response::new(proto::session::plan_session_response(value)))
            }
            PlannedSession::Dispatch(_) => Err(Status::internal(
                "dry-run application operation returned a dispatch plan",
            )),
        }
    }

    type DispatchSessionStream = ResponseStream<pb::DispatchSessionResponse>;

    async fn dispatch_session(
        &self,
        request: Request<Streaming<pb::DispatchSessionRequest>>,
    ) -> Result<Response<Self::DispatchSessionStream>, Status> {
        let mut inbound = request.into_inner();
        let mut start = next_start(&mut inbound).await?;
        let environment = process_dispatch_environment(&mut start)?;
        let command = proto::session::dispatch_session_request(start)?;
        let (outbound, receiver) = mpsc::channel(STREAM_BUFFER);
        let mut confirmation = GrpcConfirmationClient::new(
            inbound,
            outbound.clone(),
            |prepared: &PreparedSessionDispatch| pb::DispatchSessionResponse {
                value: Some(pb::dispatch_session_response::Value::Preflight(
                    proto::session::dispatch_session_preflight(prepared),
                )),
            },
            |message| match message.value {
                Some(pb::dispatch_session_request::Value::Decision(decision)) => {
                    Ok(decision.confirmed)
                }
                Some(pb::dispatch_session_request::Value::Start(_)) | None => {
                    Err(ConfirmationClientError::UnexpectedMessage)
                }
            },
        )
        .with_mutation_lock(self.state.task_mutations.clone());
        let state = self.state.clone();
        let agent = AgentHarness::new(environment.clone());
        tokio::spawn(async move {
            let clients = SessionPlanningClients::new(agent, state.project_directory);
            let result = dispatch_confirmed_session::execute(
                &command,
                &state.store,
                &state.pool,
                &state.home,
                &clients,
                &mut confirmation,
            )
            .await;
            let item = result
                .map(proto::session::dispatch_session_result)
                .map(|result| pb::DispatchSessionResponse {
                    value: Some(pb::dispatch_session_response::Value::Result(result)),
                })
                .map_err(dispatch_confirmed_status);
            let _ = outbound.send(item).await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }
}

fn process_environment(request: &mut pb::PlanSessionRequest) -> Result<ProcessEnvironment, Status> {
    let values = std::mem::take(&mut request.environment);
    validate_process_environment(values)
}

fn process_dispatch_environment(
    request: &mut pb::DispatchSessionStart,
) -> Result<ProcessEnvironment, Status> {
    let values = std::mem::take(&mut request.environment);
    validate_process_environment(values)
}

fn validate_process_environment(
    values: std::collections::HashMap<String, String>,
) -> Result<ProcessEnvironment, Status> {
    if values.is_empty() {
        return Ok(ProcessEnvironment::inherited());
    }
    if values.len() > ENVIRONMENT_VARIABLES_MAX {
        return Err(Status::invalid_argument(format!(
            "environment may contain at most {ENVIRONMENT_VARIABLES_MAX} variables"
        )));
    }
    for (key, value) in &values {
        if key.is_empty() || key.contains(['=', '\0']) || key.len() > ENVIRONMENT_KEY_BYTES_MAX {
            return Err(Status::invalid_argument(
                "environment contains an invalid variable name",
            ));
        }
        if value.contains('\0') || value.len() > ENVIRONMENT_VALUE_BYTES_MAX {
            return Err(Status::invalid_argument(format!(
                "environment value for {key} is invalid or exceeds {ENVIRONMENT_VALUE_BYTES_MAX} bytes"
            )));
        }
    }
    Ok(ProcessEnvironment::new(values))
}

async fn next_start(
    inbound: &mut Streaming<pb::DispatchSessionRequest>,
) -> Result<pb::DispatchSessionStart, Status> {
    let message = inbound
        .message()
        .await?
        .ok_or_else(|| Status::invalid_argument("session stream requires a start message"))?;
    match message.value {
        Some(pb::dispatch_session_request::Value::Start(start)) => Ok(start),
        Some(pb::dispatch_session_request::Value::Decision(_)) | None => Err(
            Status::invalid_argument("session stream must start with start"),
        ),
    }
}

fn dispatch_confirmed_status(error: DispatchConfirmedSessionError) -> Status {
    match error {
        DispatchConfirmedSessionError::Plan(error) => plan_session_status(&error),
        DispatchConfirmedSessionError::Dispatch(error) => dispatch_session_status(&error),
        DispatchConfirmedSessionError::Confirmation(error) => confirmation_status(&error),
        DispatchConfirmedSessionError::Mutation(error) => task_mutation_status(&error),
        DispatchConfirmedSessionError::DryRunPlan => Status::internal(error.to_string()),
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

fn plan_session_status(error: &PlanSessionError) -> Status {
    let message = error.to_string();
    match error {
        PlanSessionError::NotLaunchable { .. }
        | PlanSessionError::ProjectSourceMissing { .. }
        | PlanSessionError::ProjectPathMissing { .. }
        | PlanSessionError::InvalidProjectPath { .. }
        | PlanSessionError::EmptyAgentCommand => Status::failed_precondition(message),
        PlanSessionError::FindTask(_)
        | PlanSessionError::ReadTaskMarkdown(_)
        | PlanSessionError::RenderThreadTitle(_) => Status::internal(message),
    }
}

fn dispatch_session_status(error: &DispatchSessionError) -> Status {
    let message = error.to_string();
    match error {
        DispatchSessionError::EmptyAgentCommand => Status::failed_precondition(message),
        DispatchSessionError::AgentPreparation { .. }
        | DispatchSessionError::NamedThreadBackend { .. } => Status::internal(message),
    }
}
