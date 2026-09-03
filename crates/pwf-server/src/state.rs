use std::sync::Arc;

use anyhow::Context as _;
use pwf_infra::{
    clock::LocalClock, obsidian::ObsidianStore, session::LocalProjectDirectoryClient,
    user_settings::TomlSettingsStore,
};
use pwf_models::project::HomeDirectory;
use sqlx::SqlitePool;

/// Cloneable composition shared by stateless gRPC service adapters.
#[derive(Clone)]
pub struct AppState {
    pub(crate) pool: SqlitePool,
    pub(crate) home: HomeDirectory,
    pub(crate) store: ObsidianStore,
    pub(crate) clock: LocalClock,
    pub(crate) project_directory: LocalProjectDirectoryClient,
    pub(crate) user_settings: TomlSettingsStore,
    pub(crate) task_mutations: Arc<tokio::sync::Mutex<()>>,
}

impl AppState {
    pub async fn from_environment() -> anyhow::Result<Self> {
        let path = pwf_infra::database::database_path()
            .context("resolving the PWF project database path")?;
        let pool = pwf_infra::database::build_pool(&path)
            .await
            .with_context(|| format!("opening the PWF project database {}", path.display()))?;
        pwf_infra::database::check_database_ready(&pool)
            .await
            .with_context(|| format!("checking PWF database readiness at {}", path.display()))?;
        let home = directories::BaseDirs::new()
            .map(|directories| HomeDirectory::new(directories.home_dir().to_path_buf()))
            .context("resolving the home directory for managed projects")?;
        Ok(Self::new(pool, home, TomlSettingsStore::from_environment()))
    }

    #[must_use]
    pub fn new(pool: SqlitePool, home: HomeDirectory, user_settings: TomlSettingsStore) -> Self {
        Self {
            store: ObsidianStore::new(home.clone()),
            pool,
            home,
            clock: LocalClock,
            project_directory: LocalProjectDirectoryClient,
            user_settings,
            task_mutations: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}
