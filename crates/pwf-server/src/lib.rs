//! Local IPC gRPC process root for PWF.

mod observability;
mod release;
mod server;
mod services;
mod state;

use std::time::Duration;

use anyhow::Context as _;
use pwf_local_transport::{LocalEndpoint, LocalListener};
pub use server::{ServerLifecycle, ServerState, serve};
pub use state::AppState;

const PRODUCTION_SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(10);

/// Runs the resident server using platform-local database, IPC, and logging state.
pub async fn run() -> anyhow::Result<()> {
    let _observability = observability::initialize()?;
    tracing::info!(event = "starting", "pwf-server lifecycle");

    let endpoint =
        LocalEndpoint::from_environment().context("resolving the local server endpoint")?;
    let listener = LocalListener::bind(&endpoint, Duration::from_secs(1))
        .await
        .context("binding the local gRPC listener")?;
    let state = AppState::from_environment().await?;

    tracing::info!(path = %endpoint.path().display(), event = "ready", "pwf-server lifecycle");
    let result = serve(
        listener,
        async {
            if let Err(error) = shutdown_signal().await {
                tracing::error!(%error, "failed while waiting for a shutdown signal");
            }
        },
        PRODUCTION_SHUTDOWN_GRACE_PERIOD,
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
