//! Local IPC gRPC process root for PWF.

pub mod doctor;
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

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("{0}\nUse a server release compatible with this database's migration history.")]
    MigrationHistory(String),
    #[error(transparent)]
    Runtime(#[from] anyhow::Error),
}

/// Runs the resident server using platform-local database, IPC, and logging state.
pub async fn run() -> Result<(), RunError> {
    let _observability = observability::initialize()?;
    tracing::info!(event = "starting", "pwf-server lifecycle");
    let result = run_resident().await;
    if let Err(error) = &result {
        tracing::error!(event = "startup_or_runtime_failure", error = %error, "pwf-server lifecycle");
    }
    result
}

async fn run_resident() -> Result<(), RunError> {
    let endpoint =
        LocalEndpoint::from_environment().context("resolving the local server endpoint")?;
    let listener = LocalListener::bind(&endpoint, Duration::from_secs(1))
        .await
        .context("binding the local gRPC listener")?;
    let path = pwf_infra::database::database_path()?;
    if path.try_exists().context("checking database path")? {
        let pool = pwf_infra::database::build_read_only_pool(&path)
            .await
            .context("opening the PWF project database for startup validation")?;
        let compatibility = pwf_infra::database::check_database_compatible(&pool).await;
        pool.close().await;
        if let pwf_infra::database::MigrationCompatibility::Incompatible { reason } = compatibility?
        {
            return Err(RunError::MigrationHistory(format!(
                "{}: {reason}",
                path.display()
            )));
        }
    }
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
    result.map_err(Into::into)
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
