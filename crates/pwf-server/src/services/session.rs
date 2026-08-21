use std::{pin::Pin, sync::Arc, time::Duration};

use futures::{Stream, future::BoxFuture};
use pwf_application::{
    contract::task::session::{PlanSessionIntent, PlannedSession, PreparedSessionDispatch},
    ports::session_confirmation::SessionConfirmationClient,
    task::session::{
        dispatch_confirmed_session::{self, DispatchConfirmedSessionError},
        dispatch_session::DispatchSessionError,
        plan_session::{self, PlanSessionError, SessionPlanningClients},
    },
};
use pwf_infra::session::{AgentHarness, ProcessEnvironment, TmuxHarness};
use pwf_wire::v1::{self, session_service_server::SessionService};
use tokio::sync::{Mutex, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};

use crate::{
    AppState,
    conversion::{input, output},
};

const CONFIRMATION_TIMEOUT: Duration = Duration::from_mins(30);
const STREAM_BUFFER: usize = 4;
const ENVIRONMENT_VARIABLES_MAX: usize = 512;
const ENVIRONMENT_KEY_BYTES_MAX: usize = 256;
const ENVIRONMENT_VALUE_BYTES_MAX: usize = 32 * 1024;

type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[derive(Clone)]
pub(crate) struct SessionApi {
    state: AppState,
}

impl SessionApi {
    pub(crate) fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl SessionService for SessionApi {
    async fn plan_session(
        &self,
        request: Request<v1::PlanSessionRequest>,
    ) -> Result<Response<v1::DryRunSession>, Status> {
        let mut request = request.into_inner();
        let environment = process_environment(&mut request)?;
        let command = input::plan_session(request)?;
        if command.intent != PlanSessionIntent::DryRun {
            return Err(Status::invalid_argument(
                "PlanSession requires PLAN_SESSION_INTENT_DRY_RUN",
            ));
        }
        let agent = AgentHarness::new(environment.clone());
        let session = TmuxHarness::new(environment);
        let clients = SessionPlanningClients::new(agent, self.state.project_directory, session);
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
            PlannedSession::DryRun(value) => Ok(Response::new(output::dry_run_session(value))),
            PlannedSession::Dispatch(_) => Err(Status::internal(
                "dry-run application operation returned a dispatch plan",
            )),
        }
    }

    type DispatchSessionStream = ResponseStream<v1::DispatchSessionServerMessage>;

    async fn dispatch_session(
        &self,
        request: Request<Streaming<v1::DispatchSessionClientMessage>>,
    ) -> Result<Response<Self::DispatchSessionStream>, Status> {
        let mut inbound = request.into_inner();
        let mut start = next_start(&mut inbound).await?;
        let environment = process_environment(&mut start)?;
        let command = input::plan_session(start)?;
        if command.intent != PlanSessionIntent::Dispatch {
            return Err(Status::invalid_argument(
                "DispatchSession requires PLAN_SESSION_INTENT_DISPATCH",
            ));
        }
        let (outbound, receiver) = mpsc::channel(STREAM_BUFFER);
        let confirmation = Arc::new(SessionConfirmation::new(inbound, outbound.clone()));
        let state = self.state.clone();
        let agent = AgentHarness::new(environment.clone());
        let session = TmuxHarness::new(environment);
        tokio::spawn(async move {
            let clients = SessionPlanningClients::new(agent, state.project_directory, session);
            let result = dispatch_confirmed_session::execute(
                &command,
                &state.store,
                &state.pool,
                &state.home,
                &clients,
                confirmation.as_ref(),
            )
            .await;
            let item = match confirmation.take_failure() {
                Some(status) => Err(status),
                None => result
                    .map(output::dispatched_session)
                    .map(|result| v1::DispatchSessionServerMessage {
                        value: Some(v1::dispatch_session_server_message::Value::Result(result)),
                    })
                    .map_err(dispatch_confirmed_status),
            };
            let _ = outbound.send(item).await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }
}

fn process_environment(request: &mut v1::PlanSessionRequest) -> Result<ProcessEnvironment, Status> {
    let values = std::mem::take(&mut request.environment);
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
    inbound: &mut Streaming<v1::DispatchSessionClientMessage>,
) -> Result<v1::PlanSessionRequest, Status> {
    let message = inbound
        .message()
        .await?
        .ok_or_else(|| Status::invalid_argument("session stream requires a start message"))?;
    match message.value {
        Some(v1::dispatch_session_client_message::Value::Start(start)) => Ok(start),
        Some(v1::dispatch_session_client_message::Value::Decision(_)) | None => Err(
            Status::invalid_argument("session stream must start with start"),
        ),
    }
}

struct SessionConfirmation {
    inbound: Mutex<Streaming<v1::DispatchSessionClientMessage>>,
    outbound: mpsc::Sender<Result<v1::DispatchSessionServerMessage, Status>>,
    failure: std::sync::Mutex<Option<Status>>,
}

impl SessionConfirmation {
    fn new(
        inbound: Streaming<v1::DispatchSessionClientMessage>,
        outbound: mpsc::Sender<Result<v1::DispatchSessionServerMessage, Status>>,
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

impl SessionConfirmationClient for SessionConfirmation {
    fn confirm<'a>(&'a self, prepared: &'a PreparedSessionDispatch) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let preflight = v1::DispatchSessionServerMessage {
                value: Some(v1::dispatch_session_server_message::Value::Preflight(
                    output::session_preflight(prepared),
                )),
            };
            if self.outbound.send(Ok(preflight)).await.is_err() {
                self.fail(Status::cancelled("session confirmation stream closed"));
                return false;
            }
            let mut inbound = self.inbound.lock().await;
            let decision = tokio::time::timeout(CONFIRMATION_TIMEOUT, inbound.message()).await;
            match decision {
                Ok(Ok(Some(message))) => match message.value {
                    Some(v1::dispatch_session_client_message::Value::Decision(decision)) => {
                        decision.confirmed
                    }
                    Some(v1::dispatch_session_client_message::Value::Start(_)) | None => {
                        self.fail(Status::invalid_argument(
                            "session stream requires one decision after preflight",
                        ));
                        false
                    }
                },
                Ok(Ok(None)) => {
                    self.fail(Status::cancelled("session confirmation stream closed"));
                    false
                }
                Ok(Err(status)) => {
                    self.fail(status);
                    false
                }
                Err(_) => {
                    self.fail(Status::deadline_exceeded("session confirmation timed out"));
                    false
                }
            }
        })
    }
}

fn dispatch_confirmed_status(error: DispatchConfirmedSessionError) -> Status {
    match error {
        DispatchConfirmedSessionError::Plan(error) => plan_session_status(&error),
        DispatchConfirmedSessionError::Dispatch(error) => dispatch_session_status(&error),
        DispatchConfirmedSessionError::DryRunPlan => Status::internal(error.to_string()),
    }
}

fn plan_session_status(error: &PlanSessionError) -> Status {
    let message = match error {
        PlanSessionError::MultiplexerSessionMissing {
            session,
            start_command_argv,
        } => format!(
            "tmux session '{session}' does not exist.\nStart it with:\n{}",
            start_command_argv
                .iter()
                .map(|argument| shell_words::quote(argument))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        _ => error.to_string(),
    };
    match error {
        PlanSessionError::NotLaunchable { .. }
        | PlanSessionError::ProjectPathMissing { .. }
        | PlanSessionError::MultiplexerNotFound
        | PlanSessionError::MultiplexerSessionMissing { .. }
        | PlanSessionError::InvalidProjectPath { .. }
        | PlanSessionError::EmptyAgentCommand => Status::failed_precondition(message),
        PlanSessionError::FindTask(_)
        | PlanSessionError::ReadTaskMarkdown(_)
        | PlanSessionError::MultiplexerSessionCheck { .. }
        | PlanSessionError::ModelTier(_)
        | PlanSessionError::RenderThreadTitle(_) => Status::internal(message),
    }
}

fn dispatch_session_status(error: &DispatchSessionError) -> Status {
    let message = error.to_string();
    match error {
        DispatchSessionError::WindowOpen { .. } => Status::unavailable(message),
        DispatchSessionError::EmptyAgentCommand => Status::failed_precondition(message),
        DispatchSessionError::AgentPreparation { .. }
        | DispatchSessionError::NamedThreadBackend { .. } => Status::internal(message),
    }
}
