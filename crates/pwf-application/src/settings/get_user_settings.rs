use pwf_models::settings::UserSettings;

use crate::ports::user_settings::{UserSettingsLoadError, UserSettingsReader};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GetUserSettings;

#[derive(Debug, thiserror::Error)]
pub enum GetUserSettingsError {
    #[error(transparent)]
    Settings(#[from] UserSettingsLoadError),
}

#[cqrsy::query]
pub fn execute(
    _query: GetUserSettings,
    settings_reader: &impl UserSettingsReader,
) -> Result<UserSettings, GetUserSettingsError> {
    Ok(settings_reader.load()?)
}
