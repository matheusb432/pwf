use std::{net::Ipv4Addr, time::Duration};

use anyhow::{Context as _, ensure};
use pwf_client::PwfClient;
use pwf_infra::user_settings::TomlSettingsStore;
use pwf_local_auth::{
    CapabilityToken, LocalAuth, PublishedEndpoint, ServerEndpoint, ServerInstanceId,
};
use pwf_models::project::HomeDirectory;
use pwf_server::{AppState, ServerLifecycle, ServerState, serve};
use tokio::sync::{oneshot, watch};
use tonic::transport::Channel;

const SERVER_OBSERVATION_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct TestServer {
    pub(crate) root: tempfile::TempDir,
    pub(crate) token: CapabilityToken,
    endpoint: ServerEndpoint,
    pub(crate) client: PwfClient,
    shutdown: Option<oneshot::Sender<()>>,
    lifecycle: watch::Receiver<ServerState>,
    task: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
    _published: PublishedEndpoint,
}

impl TestServer {
    pub(crate) async fn start(shutdown_grace_period: Duration) -> anyhow::Result<Self> {
        let root = tempfile::tempdir()?;
        let database_path = root.path().join("pwf.sqlite3");
        let migration_pool = pwf_infra::database::build_migration_pool(&database_path).await?;
        pwf_infra::database::migrate_database(&migration_pool).await?;
        migration_pool.close().await;
        let pool = pwf_infra::database::build_pool(&database_path).await?;
        let home_path = root.path().join("home");
        std::fs::create_dir_all(&home_path)?;
        let state = AppState::new(
            pool,
            HomeDirectory::new(home_path),
            TomlSettingsStore::new(Some(root.path().join("config.toml"))),
        );

        let auth = LocalAuth::from_data_root(root.path().join("auth"))?;
        let token = auth.load_or_create_server_token()?;
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let endpoint =
            ServerEndpoint::try_new(listener.local_addr()?, ServerInstanceId::generate())?;
        let published = auth.publish_endpoint(endpoint.clone())?;
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let (lifecycle_handle, mut lifecycle) = ServerLifecycle::channel();
        let task = tokio::spawn(serve(
            listener,
            async move {
                let _ = shutdown_receiver.await;
            },
            shutdown_grace_period,
            token.clone(),
            state,
            lifecycle_handle,
        ));
        wait_for_state(&mut lifecycle, ServerState::Serving).await?;
        let client = PwfClient::connect(&auth).await?;

        Ok(Self {
            root,
            token,
            endpoint,
            client,
            shutdown: Some(shutdown_sender),
            lifecycle,
            task: Some(task),
            _published: published,
        })
    }

    pub(crate) async fn channel(&self) -> anyhow::Result<Channel> {
        Ok(
            tonic::transport::Endpoint::from_shared(format!("http://{}", self.endpoint.address()))?
                .connect()
                .await?,
        )
    }

    pub(crate) fn begin_shutdown(&mut self) -> anyhow::Result<()> {
        let shutdown = self
            .shutdown
            .take()
            .context("server shutdown sender is missing")?;
        shutdown
            .send(())
            .map_err(|()| anyhow::anyhow!("server shutdown receiver is closed"))
    }

    pub(crate) async fn wait_for(&mut self, state: ServerState) -> anyhow::Result<()> {
        wait_for_state(&mut self.lifecycle, state).await
    }

    pub(crate) async fn finish(mut self) -> anyhow::Result<()> {
        if self.shutdown.is_some() {
            self.begin_shutdown()?;
        }
        let task = self.task.take().context("server task is missing")?;
        let outcome = tokio::time::timeout(SERVER_OBSERVATION_TIMEOUT, task).await?;
        outcome??;
        ensure!(
            *self.lifecycle.borrow() == ServerState::Stopped,
            "server lifecycle did not reach stopped"
        );
        Ok(())
    }
}

async fn wait_for_state(
    lifecycle: &mut watch::Receiver<ServerState>,
    expected: ServerState,
) -> anyhow::Result<()> {
    tokio::time::timeout(
        SERVER_OBSERVATION_TIMEOUT,
        observe_state(lifecycle, expected),
    )
    .await??;
    Ok(())
}

async fn observe_state(
    lifecycle: &mut watch::Receiver<ServerState>,
    expected: ServerState,
) -> anyhow::Result<()> {
    loop {
        if *lifecycle.borrow_and_update() == expected {
            return Ok(());
        }
        lifecycle.changed().await?;
    }
}
