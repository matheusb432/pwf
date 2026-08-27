//! Authenticated loopback gRPC process root for PWF.

mod observability;
mod server;
mod services;
mod state;

use std::{net::Ipv4Addr, time::Duration};

use anyhow::Context as _;
use pwf_local_auth::{LocalAuth, ServerEndpoint, ServerInstanceId};
pub use server::{ServerLifecycle, ServerState, serve};
pub use state::AppState;

const PRODUCTION_SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(10);

/// Runs the resident server using platform-local database, auth, and logging state.
pub async fn run() -> anyhow::Result<()> {
    let _observability = observability::initialize()?;
    tracing::info!(event = "starting", "pwf-server lifecycle");

    let auth = LocalAuth::from_environment().context("resolving local server state")?;
    let capability = auth
        .load_or_create_server_token()
        .context("provisioning local server capability")?;
    let state = AppState::from_environment().await?;
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .context("binding the loopback gRPC listener")?;
    let address = listener
        .local_addr()
        .context("reading the bound gRPC address")?;
    let endpoint = ServerEndpoint::try_new(address, ServerInstanceId::generate())
        .context("validating the bound gRPC endpoint")?;
    let _published = auth
        .publish_endpoint(endpoint.clone())
        .context("publishing the local gRPC endpoint")?;

    tracing::info!(
        address = %endpoint.address(),
        instance_id = %endpoint.instance_id(),
        event = "ready",
        "pwf-server lifecycle"
    );
    let result = serve(
        listener,
        async {
            if let Err(error) = shutdown_signal().await {
                tracing::error!(%error, "failed while waiting for a shutdown signal");
            }
        },
        PRODUCTION_SHUTDOWN_GRACE_PERIOD,
        capability,
        state,
        ServerLifecycle::default(),
    )
    .await;
    tracing::info!(event = "stopped", "pwf-server lifecycle");
    result
}

async fn shutdown_signal() -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .context("installing the SIGTERM handler")?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.context("waiting for SIGINT"),
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .context("waiting for the shutdown signal")
    }
}
