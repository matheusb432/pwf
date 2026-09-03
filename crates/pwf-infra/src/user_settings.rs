//! TOML-backed user settings used by the server process root.

use std::{path::PathBuf, str::FromStr as _};

use pwf_application::ports::user_settings::{
    UserSettingsConfigurationError, UserSettingsLoadError, UserSettingsReader,
};
use pwf_models::settings::{RgbColor, RgbColorError, TaskStatusColors, UserSettings};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct UserSettingsDocument {
    colors: TaskStatusColorsDocument,
}

impl UserSettingsDocument {
    fn parse(bytes: Vec<u8>) -> Result<Self, UserSettingsDocumentError> {
        let source = String::from_utf8(bytes)?;
        toml::from_str(&source).map_err(UserSettingsDocumentError::TomlSchema)
    }

    fn into_settings(self) -> Result<UserSettings, UserSettingsDocumentError> {
        Ok(UserSettings::new(TaskStatusColors::new(
            configured_color("colors.active", self.colors.active)?,
            configured_color("colors.done", self.colors.done)?,
            configured_color("colors.cancelled", self.colors.cancelled)?,
        )))
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct TaskStatusColorsDocument {
    active: Option<String>,
    done: Option<String>,
    cancelled: Option<String>,
}

fn configured_color(
    key: &'static str,
    value: Option<String>,
) -> Result<Option<RgbColor>, UserSettingsDocumentError> {
    value
        .map(|value| {
            RgbColor::from_str(&value)
                .map_err(|source| UserSettingsDocumentError::Color { key, source })
        })
        .transpose()
}

#[derive(Debug, thiserror::Error)]
enum UserSettingsDocumentError {
    #[error("user settings are not UTF-8: {0}")]
    Encoding(#[from] std::string::FromUtf8Error),
    #[error("user settings TOML schema is invalid: {0}")]
    TomlSchema(#[source] toml::de::Error),
    #[error("`{key}` is invalid: {source}")]
    Color {
        key: &'static str,
        #[source]
        source: RgbColorError,
    },
}

/// TOML-backed user settings for one resolved configuration path.
#[derive(Clone, Debug)]
pub struct TomlSettingsStore {
    path: Option<PathBuf>,
}

impl TomlSettingsStore {
    #[must_use]
    pub const fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    #[must_use]
    pub fn from_environment() -> Self {
        let path = directories::BaseDirs::new()
            .map(|directories| directories.config_dir().join("pwf").join("config.toml"));
        Self::new(path)
    }
}

impl UserSettingsReader for TomlSettingsStore {
    fn load(&self) -> Result<UserSettings, UserSettingsLoadError> {
        let Some(path) = self.path.as_deref() else {
            return Ok(UserSettings::default());
        };
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(UserSettings::default());
            }
            Err(error) => {
                return Err(anyhow::Error::new(error)
                    .context(format!("reading user settings {}", path.display()))
                    .into());
            }
        };
        let settings = UserSettingsDocument::parse(bytes)
            .and_then(UserSettingsDocument::into_settings)
            .map_err(|source| {
                UserSettingsConfigurationError::new(path.to_path_buf(), anyhow::Error::new(source))
            })?;
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pwf_application::ports::user_settings::{UserSettingsLoadError, UserSettingsReader};
    use pwf_models::settings::{RgbColor, UserSettings};

    use super::TomlSettingsStore;

    #[test]
    fn partial_document_overrides_only_the_configured_status_color() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, "[colors]\nactive = \"#ff8700\"\n").unwrap();
        let store = TomlSettingsStore::new(Some(path));

        let colors = store.load().unwrap().task_status_colors();

        assert_eq!(colors.active(), Some(RgbColor::new(255, 135, 0)));
        assert_eq!(colors.done(), None);
        assert_eq!(colors.cancelled(), None);
    }

    #[test]
    fn missing_document_returns_defaults_without_creating_a_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing").join("config.toml");
        let store = TomlSettingsStore::new(Some(path.clone()));

        assert_eq!(store.load().unwrap(), UserSettings::default());
        assert!(!path.exists());
    }

    #[test]
    fn invalid_documents_report_their_path_and_cause() -> anyhow::Result<()> {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let store = TomlSettingsStore::new(Some(path.clone()));

        for source in [
            "unknown = true\n",
            "[colors]\nunknown = \"#ffffff\"\n",
            "[colors]\nactive = \"#fff\"\n",
            "[colors]\nactive = 42\n",
        ] {
            fs::write(&path, source).unwrap();
            let error = store.load().unwrap_err();
            let UserSettingsLoadError::InvalidConfiguration(error) = error else {
                anyhow::bail!("expected invalid configuration, got {error:?}");
            };

            assert_eq!(error.path(), path);
            assert!(error.to_string().contains("config.toml"));
        }

        Ok(())
    }

    #[test]
    fn each_load_reads_the_latest_document() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let store = TomlSettingsStore::new(Some(path.clone()));
        fs::write(&path, "[colors]\nactive = \"#010203\"\n").unwrap();
        assert_eq!(
            store.load().unwrap().task_status_colors().active(),
            Some(RgbColor::new(1, 2, 3))
        );

        fs::write(&path, "[colors]\nactive = \"#040506\"\n").unwrap();
        assert_eq!(
            store.load().unwrap().task_status_colors().active(),
            Some(RgbColor::new(4, 5, 6))
        );
    }
}
