use std::{future::Future, time::Duration};

use anyhow::Context as _;
use pwf_local_transport::LocalListener;
use pwf_wire::{FILE_DESCRIPTOR_SET, pb};
use tonic::{server::NamedService, transport::Server};
use tower_http::{
    LatencyUnit,
    trace::{DefaultMakeSpan, DefaultOnEos, DefaultOnFailure, DefaultOnResponse, TraceLayer},
};

use crate::{
    AppState,
    services::{
        NoteGrpcService, ProjectGrpcService, SessionGrpcService, SettingsGrpcService,
        TaskGrpcService,
    },
};

const MAX_CONCURRENT_REQUESTS_PER_CONNECTION: usize = 16;
const MAX_REQUEST_DURATION: Duration = Duration::from_mins(30);
const MAX_REQUEST_MESSAGE_SIZE: usize = 64 * 1024;
const MAX_RESPONSE_MESSAGE_SIZE: usize = 4 * 1024 * 1024;
const APPLICATION_SERVICE_NAMES: [&str; 5] = [
    pb::project_service_server::ProjectServiceServer::<ProjectGrpcService>::NAME,
    pb::note_service_server::NoteServiceServer::<NoteGrpcService>::NAME,
    pb::task_service_server::TaskServiceServer::<TaskGrpcService>::NAME,
    pb::session_service_server::SessionServiceServer::<SessionGrpcService>::NAME,
    pb::settings_service_server::SettingsServiceServer::<SettingsGrpcService>::NAME,
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

pub async fn serve(
    listener: LocalListener,
    shutdown: impl Future<Output = ()> + Send + 'static,
    shutdown_grace_period: Duration,
    state: AppState,
    lifecycle: ServerLifecycle,
) -> anyhow::Result<()> {
    let (incoming, _ownership) = listener.into_parts();
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
        SessionGrpcService::new(state.clone())
    ));
    let settings_server =
        bounded_service!(pb::settings_service_server::SettingsServiceServer::new(
            SettingsGrpcService::new(state.clone())
        ));
    let reflection_server = bounded_service!(
        tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
            .register_encoded_file_descriptor_set(tonic_health::pb::FILE_DESCRIPTOR_SET)
            .build_v1()
            .context("building the gRPC reflection service")?
    );

    let shutdown_lifecycle = lifecycle.clone();
    let (snapshot_shutdown, snapshot_shutdown_receiver) = tokio::sync::watch::channel(false);
    let snapshot_worker = tokio::spawn(crate::project_snapshots::run(
        state,
        crate::project_snapshots::REFRESH_INTERVAL,
        snapshot_shutdown_receiver,
    ));
    let shutdown_snapshots = snapshot_shutdown.clone();
    let (shutdown_started_sender, shutdown_started_receiver) = tokio::sync::oneshot::channel();
    let shutdown = async move {
        shutdown.await;
        shutdown_snapshots.send_replace(true);
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
        .layer(crate::release::ReleaseLayer)
        .concurrency_limit_per_connection(MAX_CONCURRENT_REQUESTS_PER_CONNECTION)
        .load_shed(true)
        .timeout(MAX_REQUEST_DURATION)
        .add_service(health_server)
        .add_service(reflection_server)
        .add_service(project_server)
        .add_service(note_server)
        .add_service(task_server)
        .add_service(session_server)
        .add_service(settings_server)
        .serve_with_incoming_shutdown(incoming, shutdown);
    tokio::pin!(grpc_server);

    let result = tokio::select! {
        result = &mut grpc_server => result,
        _ = shutdown_started_receiver => {
            if let Ok(result) = tokio::time::timeout(shutdown_grace_period, &mut grpc_server).await {
                result
            } else {
                tracing::warn!(
                    shutdown_grace_period = ?shutdown_grace_period,
                    "gRPC connections exceeded the shutdown grace period"
                );
                Ok(())
            }
        }
    };

    snapshot_shutdown.send_replace(true);
    if let Err(error) = snapshot_worker.await {
        tracing::error!(%error, "joining project snapshot worker during shutdown");
    }
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
