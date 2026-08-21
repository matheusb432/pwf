//! Authenticated Tonic client for the resident local `pwf-server`.

use std::time::Duration;

use pwf_local_auth::{CapabilityToken, LocalAuth, LocalAuthError, ServerEndpoint};
use tonic::{
    Request, Status,
    metadata::{Ascii, MetadataValue},
    service::{Interceptor, interceptor::InterceptedService},
    transport::{Channel, Endpoint},
};
use tonic_health::pb::{HealthCheckRequest, health_client::HealthClient};

pub mod note;
pub mod project;
pub mod task;

pub use pwf_wire::v1;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(5);
const OPERATION_TIMEOUT: Duration = Duration::from_mins(30);
const MAX_REQUEST_MESSAGE_SIZE: usize = 64 * 1024;
const MAX_RESPONSE_MESSAGE_SIZE: usize = 4 * 1024 * 1024;
const AUTHORIZATION_METADATA_KEY: &str = "authorization";

pub(crate) type AuthenticatedChannel = InterceptedService<Channel, RequestPolicy>;

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error(transparent)]
    LocalBootstrap(#[from] LocalAuthError),
    #[error("local pwf-server endpoint is not a valid URI")]
    InvalidEndpoint(#[source] tonic::transport::Error),
    #[error("local capability token cannot be encoded as gRPC metadata")]
    AuthorizationMetadata(#[source] tonic::metadata::errors::InvalidMetadataValue),
    #[error("could not connect to local pwf-server")]
    Transport(#[source] tonic::transport::Error),
    #[error("local pwf-server health check failed")]
    Health(#[source] Status),
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("pwf-server request failed: {0}")]
    Rpc(#[from] Status),
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
    endpoint: ServerEndpoint,
}

impl PwfClient {
    /// Discovers and authenticates the OS-managed local server.
    pub async fn connect_local() -> Result<Self, ConnectError> {
        let auth = LocalAuth::from_environment()?;
        Self::connect(&auth).await
    }

    /// Discovers and authenticates the server published in `auth`'s data root.
    pub async fn connect(auth: &LocalAuth) -> Result<Self, ConnectError> {
        let endpoint = auth.load_endpoint()?;
        let token = auth.load_client_token()?;
        let client = Self::connect_endpoint(&endpoint, &token).await?;
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
        project::ProjectClient::new(self.channel.clone(), self.request_policy.clone())
    }

    #[must_use]
    pub fn note(&self) -> note::NoteClient {
        note::NoteClient::new(self.channel.clone(), self.request_policy.clone())
    }

    #[must_use]
    pub fn task(&self) -> task::TaskClient {
        task::TaskClient::new(self.channel.clone(), self.request_policy.clone())
    }

    #[must_use]
    pub const fn endpoint(&self) -> &ServerEndpoint {
        &self.endpoint
    }

    async fn connect_endpoint(
        endpoint: &ServerEndpoint,
        token: &CapabilityToken,
    ) -> Result<Self, ConnectError> {
        let channel_endpoint = Endpoint::from_shared(format!("http://{}", endpoint.address()))
            .map_err(ConnectError::InvalidEndpoint)?
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(OPERATION_TIMEOUT);
        let channel = channel_endpoint
            .connect()
            .await
            .map_err(ConnectError::Transport)?;
        let request_policy = RequestPolicy::try_new(token)?;
        Ok(Self {
            channel,
            request_policy,
            endpoint: endpoint.clone(),
        })
    }

    async fn check_health_inner(&self) -> Result<(), Status> {
        let mut client =
            HealthClient::with_interceptor(self.channel.clone(), self.request_policy.clone())
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

#[derive(Clone)]
pub(crate) struct RequestPolicy {
    value: MetadataValue<Ascii>,
}

impl std::fmt::Debug for RequestPolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RequestPolicy(REDACTED)")
    }
}

impl RequestPolicy {
    fn try_new(token: &CapabilityToken) -> Result<Self, ConnectError> {
        let value = format!("Bearer {}", token.expose_secret())
            .parse()
            .map_err(ConnectError::AuthorizationMetadata)?;
        Ok(Self { value })
    }
}

impl Interceptor for RequestPolicy {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        request
            .metadata_mut()
            .insert(AUTHORIZATION_METADATA_KEY, self.value.clone());
        if request.metadata().get("grpc-timeout").is_none() {
            request.set_timeout(OPERATION_TIMEOUT);
        }
        Ok(request)
    }
}
