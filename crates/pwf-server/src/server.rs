use std::{future::Future, time::Duration};

use anyhow::Context as _;
use pwf_local_auth::CapabilityToken;
use pwf_wire::{FILE_DESCRIPTOR_SET, pb};
use tonic::{
    Request, Status,
    server::NamedService,
    service::{Interceptor, InterceptorLayer},
    transport::{Server, server::TcpIncoming},
};
use tower_http::{
    LatencyUnit,
    trace::{DefaultMakeSpan, DefaultOnEos, DefaultOnFailure, DefaultOnResponse, TraceLayer},
};

use crate::{
    AppState,
    services::{NoteGrpcService, ProjectGrpcService, SessionGrpcService, TaskGrpcService},
};

const MAX_CONCURRENT_REQUESTS_PER_CONNECTION: usize = 16;
const MAX_REQUEST_DURATION: Duration = Duration::from_mins(30);
const MAX_REQUEST_MESSAGE_SIZE: usize = 64 * 1024;
const MAX_RESPONSE_MESSAGE_SIZE: usize = 4 * 1024 * 1024;
const AUTHORIZATION_METADATA_KEY: &str = "authorization";
const AUTHORIZATION_SCHEME: &str = "Bearer ";
const APPLICATION_SERVICE_NAMES: [&str; 4] = [
    pb::project_service_server::ProjectServiceServer::<ProjectGrpcService>::NAME,
    pb::note_service_server::NoteServiceServer::<NoteGrpcService>::NAME,
    pb::task_service_server::TaskServiceServer::<TaskGrpcService>::NAME,
    pb::session_service_server::SessionServiceServer::<SessionGrpcService>::NAME,
];

macro_rules! bounded_service {
    ($service:expr) => {
        $service
            .max_decoding_message_size(MAX_REQUEST_MESSAGE_SIZE)
            .max_encoding_message_size(MAX_RESPONSE_MESSAGE_SIZE)
    };
}

macro_rules! grpc_trace_layer {
    () => {
        TraceLayer::new_for_grpc()
            .make_span_with(
                DefaultMakeSpan::new()
                    .level(tracing::Level::INFO)
                    .include_headers(false),
            )
            .on_request(())
            .on_response(
                DefaultOnResponse::new()
                    .level(tracing::Level::INFO)
                    .latency_unit(LatencyUnit::Micros)
                    .include_headers(false),
            )
            .on_eos(
                DefaultOnEos::new()
                    .level(tracing::Level::INFO)
                    .latency_unit(LatencyUnit::Micros),
            )
            .on_failure(
                DefaultOnFailure::new()
                    .level(tracing::Level::WARN)
                    .latency_unit(LatencyUnit::Micros),
            )
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerState {
    Serving,
    NotServing,
    Stopped,
}

/// Optional lifecycle observation used by real-server tests and process supervision.
#[derive(Debug, Clone, Default)]
pub struct ServerLifecycle {
    sender: Option<tokio::sync::watch::Sender<ServerState>>,
}

impl ServerLifecycle {
    #[must_use]
    pub fn channel() -> (Self, tokio::sync::watch::Receiver<ServerState>) {
        let (sender, receiver) = tokio::sync::watch::channel(ServerState::Stopped);
        (
            Self {
                sender: Some(sender),
            },
            receiver,
        )
    }

    fn publish(&self, state: ServerState) {
        if let Some(sender) = &self.sender {
            sender.send_replace(state);
        }
    }
}

#[derive(Clone)]
struct Authentication {
    capability: CapabilityToken,
}

impl std::fmt::Debug for Authentication {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Authentication(REDACTED)")
    }
}

impl Interceptor for Authentication {
    fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status> {
        let authenticated = request
            .metadata()
            .get(AUTHORIZATION_METADATA_KEY)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix(AUTHORIZATION_SCHEME))
            .is_some_and(|candidate| self.capability.authenticates(candidate));
        if !authenticated {
            return Err(Status::unauthenticated("authentication required"));
        }
        Ok(request)
    }
}

pub async fn serve(
    listener: tokio::net::TcpListener,
    shutdown: impl Future<Output = ()> + Send + 'static,
    shutdown_grace_period: Duration,
    capability: CapabilityToken,
    state: AppState,
    lifecycle: ServerLifecycle,
) -> anyhow::Result<()> {
    let incoming = TcpIncoming::from(listener).with_nodelay(Some(true));
    let (health_reporter, health_server) = tonic_health::server::health_reporter();
    publish_health(
        &health_reporter,
        &APPLICATION_SERVICE_NAMES,
        tonic_health::ServingStatus::Serving,
    )
    .await;

    let health_server = bounded_service!(health_server);
    let project_server = bounded_service!(pb::project_service_server::ProjectServiceServer::new(
        ProjectGrpcService::new(state.clone())
    ));
    let note_server = bounded_service!(pb::note_service_server::NoteServiceServer::new(
        NoteGrpcService::new(state.clone())
    ));
    let task_server = bounded_service!(pb::task_service_server::TaskServiceServer::new(
        TaskGrpcService::new(state.clone())
    ));
    let session_server = bounded_service!(pb::session_service_server::SessionServiceServer::new(
        SessionGrpcService::new(state)
    ));
    let reflection_server = bounded_service!(
        tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
            .register_encoded_file_descriptor_set(tonic_health::pb::FILE_DESCRIPTOR_SET)
            .build_v1()
            .context("building the gRPC reflection service")?
    );

    let shutdown_lifecycle = lifecycle.clone();
    let (shutdown_started_sender, shutdown_started_receiver) = tokio::sync::oneshot::channel();
    let shutdown = async move {
        shutdown.await;
        publish_health(
            &health_reporter,
            &APPLICATION_SERVICE_NAMES,
            tonic_health::ServingStatus::NotServing,
        )
        .await;
        shutdown_lifecycle.publish(ServerState::NotServing);
        let _ = shutdown_started_sender.send(());
    };
    let trace_layer = grpc_trace_layer!();

    lifecycle.publish(ServerState::Serving);
    let grpc_server = Server::builder()
        .layer(trace_layer)
        .concurrency_limit_per_connection(MAX_CONCURRENT_REQUESTS_PER_CONNECTION)
        .load_shed(true)
        .timeout(MAX_REQUEST_DURATION)
        .layer(InterceptorLayer::new(Authentication { capability }))
        .add_service(health_server)
        .add_service(reflection_server)
        .add_service(project_server)
        .add_service(note_server)
        .add_service(task_server)
        .add_service(session_server)
        .serve_with_incoming_shutdown(incoming, shutdown);
    tokio::pin!(grpc_server);

    let result = tokio::select! {
        result = &mut grpc_server => result,
        _ = shutdown_started_receiver => {
            let Ok(result) = tokio::time::timeout(shutdown_grace_period, &mut grpc_server).await else {
                tracing::warn!(
                    shutdown_grace_period = ?shutdown_grace_period,
                    "gRPC connections exceeded the shutdown grace period"
                );
                lifecycle.publish(ServerState::Stopped);
                return Ok(());
            };
            result
        }
    };

    lifecycle.publish(ServerState::Stopped);
    result.context("gRPC server failure")
}

async fn publish_health(
    reporter: &tonic_health::server::HealthReporter,
    service_names: &[&str],
    status: tonic_health::ServingStatus,
) {
    for service_name in service_names {
        reporter.set_service_status(service_name, status).await;
    }
    reporter.set_service_status("", status).await;
}
