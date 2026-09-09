//! Tonic client for the resident local `pwf-server`.

use std::time::Duration;

use pwf_local_transport::LocalEndpoint;
pub use pwf_wire::proto::task::{DecodeGetTaskDagResponseError, DecodeGetTaskResponseError};
use tonic::{
    Request, Status,
    service::{Interceptor, interceptor::InterceptedService},
    transport::Channel,
};
use tonic_health::pb::{HealthCheckRequest, health_client::HealthClient};

pub mod confirmation;
pub mod note;
pub mod project;
mod release;
pub mod settings;
pub mod task;

pub use pwf_wire::pb;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(5);
const OPERATION_TIMEOUT: Duration = Duration::from_mins(30);
const MAX_REQUEST_MESSAGE_SIZE: usize = 64 * 1024;
const MAX_RESPONSE_MESSAGE_SIZE: usize = 4 * 1024 * 1024;

pub(crate) type PolicyChannel = InterceptedService<release::ReleaseChannel, RequestPolicy>;

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("could not resolve the local pwf-server endpoint")]
    LocalEndpoint(#[from] std::io::Error),
    #[error(transparent)]
    Transport(#[from] pwf_local_transport::ConnectError),
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("pwf-server returned an invalid task: {0}")]
    InvalidTaskResponse(#[from] DecodeGetTaskResponseError),
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

#[derive(Debug)]
pub struct ServerHealth {
    pub serving: bool,
    pub version: Option<String>,
}

impl PwfClient {
    /// Connects to the OS-protected local server without a health RPC.
    pub async fn connect_local() -> Result<Self, ConnectError> {
        Self::connect(&LocalEndpoint::from_environment()?).await
    }

    pub async fn connect(endpoint: &LocalEndpoint) -> Result<Self, ConnectError> {
        let channel = endpoint.connect(CONNECT_TIMEOUT, OPERATION_TIMEOUT).await?;
        Ok(Self {
            channel,
            request_policy: RequestPolicy,
            endpoint: endpoint.clone(),
        })
    }

    pub async fn check_health(&self) -> Result<(), ClientError> {
        let health = self.health().await?;
        if !health.serving {
            return Err(Status::unavailable("pwf-server is not serving").into());
        }
        if health.version.as_deref() != Some(env!("CARGO_PKG_VERSION")) {
            return Err(
                Status::failed_precondition("pwf-server version differs from the client").into(),
            );
        }
        Ok(())
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

    pub async fn health(&self) -> Result<ServerHealth, ClientError> {
        let mut client = HealthClient::with_interceptor(self.channel.clone(), self.request_policy)
            .max_encoding_message_size(MAX_REQUEST_MESSAGE_SIZE)
            .max_decoding_message_size(MAX_RESPONSE_MESSAGE_SIZE);
        let mut request = Request::new(HealthCheckRequest {
            service: String::new(),
        });
        request.set_timeout(HEALTH_TIMEOUT);
        let response = client.check(request).await?;
        let version = response
            .metadata()
            .get("pwf-server-version")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        Ok(ServerHealth {
            serving: response.into_inner().status == tonic_health::ServingStatus::Serving as i32,
            version,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RequestPolicy;

impl Interceptor for RequestPolicy {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        request.metadata_mut().insert(
            "pwf-client-version",
            tonic::metadata::MetadataValue::from_static(env!("CARGO_PKG_VERSION")),
        );
        if request.metadata().get("grpc-timeout").is_none() {
            request.set_timeout(OPERATION_TIMEOUT);
        }
        Ok(request)
    }
}
