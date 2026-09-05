//! Tonic client for the resident local `pwf-server`.

use std::time::Duration;

use pwf_local_transport::LocalEndpoint;
pub use pwf_wire::proto::task::DecodeGetTaskDagResponseError;
use tonic::{
    Request, Status,
    service::{Interceptor, interceptor::InterceptedService},
    transport::Channel,
};
use tonic_health::pb::{HealthCheckRequest, health_client::HealthClient};

pub mod confirmation;
pub mod note;
pub mod project;
pub mod settings;
pub mod task;

pub use pwf_wire::pb;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(5);
const OPERATION_TIMEOUT: Duration = Duration::from_mins(30);
const MAX_REQUEST_MESSAGE_SIZE: usize = 64 * 1024;
const MAX_RESPONSE_MESSAGE_SIZE: usize = 4 * 1024 * 1024;

pub(crate) type PolicyChannel = InterceptedService<Channel, RequestPolicy>;

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("could not resolve the local pwf-server endpoint")]
    LocalEndpoint(#[from] std::io::Error),
    #[error(transparent)]
    Transport(#[from] pwf_local_transport::ConnectError),
    #[error("local pwf-server health check failed")]
    Health(#[source] Status),
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("pwf-server request failed: {0}")]
    Rpc(#[from] Status),
    #[error("pwf-server returned an invalid task dependency graph: {0}")]
    InvalidTaskDagResponse(#[from] DecodeGetTaskDagResponseError),
}

#[must_use]
pub fn render_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|argument| shell_words::quote(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

pub struct PwfClient {
    channel: Channel,
    request_policy: RequestPolicy,
    endpoint: LocalEndpoint,
}

impl PwfClient {
    /// Connects to the OS-protected local server and checks its readiness.
    pub async fn connect_local() -> Result<Self, ConnectError> {
        Self::connect(&LocalEndpoint::from_environment()?).await
    }

    pub async fn connect(endpoint: &LocalEndpoint) -> Result<Self, ConnectError> {
        let channel = endpoint.connect(CONNECT_TIMEOUT, OPERATION_TIMEOUT).await?;
        let client = Self {
            channel,
            request_policy: RequestPolicy,
            endpoint: endpoint.clone(),
        };
        client
            .check_health_inner()
            .await
            .map_err(ConnectError::Health)?;
        Ok(client)
    }

    pub async fn check_health(&self) -> Result<(), ClientError> {
        self.check_health_inner().await.map_err(ClientError::from)
    }

    #[must_use]
    pub fn project(&self) -> project::ProjectClient {
        project::ProjectClient::new(self.channel.clone(), self.request_policy)
    }

    #[must_use]
    pub fn note(&self) -> note::NoteClient {
        note::NoteClient::new(self.channel.clone(), self.request_policy)
    }

    #[must_use]
    pub fn task(&self) -> task::TaskClient {
        task::TaskClient::new(self.channel.clone(), self.request_policy)
    }

    #[must_use]
    pub fn settings(&self) -> settings::SettingsClient {
        settings::SettingsClient::new(self.channel.clone(), self.request_policy)
    }

    #[must_use]
    pub const fn endpoint(&self) -> &LocalEndpoint {
        &self.endpoint
    }

    async fn check_health_inner(&self) -> Result<(), Status> {
        let mut client = HealthClient::with_interceptor(self.channel.clone(), self.request_policy)
            .max_encoding_message_size(MAX_REQUEST_MESSAGE_SIZE)
            .max_decoding_message_size(MAX_RESPONSE_MESSAGE_SIZE);
        let mut request = Request::new(HealthCheckRequest {
            service: String::new(),
        });
        request.set_timeout(HEALTH_TIMEOUT);
        let response = client.check(request).await?.into_inner();
        if response.status != tonic_health::ServingStatus::Serving as i32 {
            return Err(Status::unavailable("pwf-server is not serving"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RequestPolicy;

impl Interceptor for RequestPolicy {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        if request.metadata().get("grpc-timeout").is_none() {
            request.set_timeout(OPERATION_TIMEOUT);
        }
        Ok(request)
    }
}
