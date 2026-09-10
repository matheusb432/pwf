use pwf_application::{
    ports::user_settings::{UserSettingsLoadError, UserSettingsReader},
    settings::{
        get_user_settings,
        get_user_settings::{GetUserSettings, GetUserSettingsError},
    },
};
use pwf_models::{
    settings::{NoteStatusColors, ProjectStatusColors, RgbColor, TaskStatusColors, UserSettings},
    task::{PriorityTier, order::OrderSpec},
};

#[derive(Clone)]
struct FixedUserSettingsReader(UserSettings);

impl UserSettingsReader for FixedUserSettingsReader {
    fn load(&self) -> Result<UserSettings, UserSettingsLoadError> {
        Ok(self.0)
    }
}

#[test]
fn query_returns_the_complete_validated_settings_snapshot() {
    let settings = UserSettings::new(
        TaskStatusColors::new(Some(RgbColor::new(255, 135, 0)), None, None),
        ProjectStatusColors::default(),
        NoteStatusColors::default(),
        PriorityTier::Medium,
        OrderSpec::default(),
    );
    let reader = FixedUserSettingsReader(settings);

    assert_eq!(
        get_user_settings::execute(GetUserSettings, &reader).unwrap(),
        settings
    );
}

#[derive(Clone)]
struct FailingUserSettingsReader;

impl UserSettingsReader for FailingUserSettingsReader {
    fn load(&self) -> Result<UserSettings, UserSettingsLoadError> {
        Err(anyhow::anyhow!("settings storage is unavailable").into())
    }
}

#[test]
fn query_preserves_settings_load_failures() {
    let error =
        get_user_settings::execute(GetUserSettings, &FailingUserSettingsReader).unwrap_err();

    assert!(matches!(
        error,
        GetUserSettingsError::Settings(UserSettingsLoadError::Adapter(_))
    ));
}
