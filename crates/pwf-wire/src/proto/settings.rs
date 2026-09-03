//! Explicit protobuf mappings for user-settings operations.

use pwf_models::settings::{RgbColor, UserSettings};

use crate::pb;

#[must_use]
pub fn get_user_settings_response(settings: UserSettings) -> pb::GetUserSettingsResponse {
    let colors = settings.task_status_colors();
    pb::GetUserSettingsResponse {
        task_status_colors: Some(pb::TaskStatusColors {
            active: colors.active().map(rgb_color),
            done: colors.done().map(rgb_color),
            cancelled: colors.cancelled().map(rgb_color),
        }),
    }
}

fn rgb_color(color: RgbColor) -> pb::RgbColor {
    pb::RgbColor {
        red: u32::from(color.red()),
        green: u32::from(color.green()),
        blue: u32::from(color.blue()),
    }
}
