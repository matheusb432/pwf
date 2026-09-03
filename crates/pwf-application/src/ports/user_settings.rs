use std::path::{Path, PathBuf};

use pwf_models::settings::UserSettings;

/// A readable settings file did not represent a supported configuration.
#[derive(Debug, thiserror::Error)]
#[error("user settings at {} are invalid: {source}", path.display())]
pub struct UserSettingsConfigurationError {
    path: PathBuf,
    #[source]
    source: anyhow::Error,
}

impl UserSettingsConfigurationError {
    #[must_use]
    pub fn new(path: PathBuf, source: anyhow::Error) -> Self {
        Self { path, source }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// A strict user-settings snapshot could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum UserSettingsLoadError {
    #[error(transparent)]
    InvalidConfiguration(#[from] UserSettingsConfigurationError),
    #[error(transparent)]
    Adapter(#[from] anyhow::Error),
}

/// Loads validated user-settings snapshots.
pub trait UserSettingsReader: Clone + Send + Sync + 'static {
    fn load(&self) -> Result<UserSettings, UserSettingsLoadError>;
}
