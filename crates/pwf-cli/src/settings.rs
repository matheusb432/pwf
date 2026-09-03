//! Loads validated presentation settings through the resident server.

use anyhow::Context as _;
use pwf_client::{pb, settings::SettingsClient};
use pwf_models::settings::{RgbColor, TaskStatusColors, UserSettings};

/// Loads the latest complete user-settings snapshot.
pub async fn load(client: &SettingsClient) -> anyhow::Result<UserSettings> {
    let response = client.get_user_settings().await.map_err(crate::rpc_error)?;
    decode(response)
}

fn decode(response: pb::GetUserSettingsResponse) -> anyhow::Result<UserSettings> {
    let colors = response
        .task_status_colors
        .context("pwf-server returned user settings without task status colors")?;
    Ok(UserSettings::new(TaskStatusColors::new(
        decode_color("active", colors.active)?,
        decode_color("done", colors.done)?,
        decode_color("cancelled", colors.cancelled)?,
    )))
}

fn decode_color(
    field: &'static str,
    color: Option<pb::RgbColor>,
) -> anyhow::Result<Option<RgbColor>> {
    color
        .map(|color| {
            Ok(RgbColor::new(
                decode_component(field, "red", color.red)?,
                decode_component(field, "green", color.green)?,
                decode_component(field, "blue", color.blue)?,
            ))
        })
        .transpose()
}

fn decode_component(field: &str, component: &str, value: u32) -> anyhow::Result<u8> {
    u8::try_from(value).with_context(|| {
        format!("pwf-server returned out-of-range {field} {component} color component {value}")
    })
}

#[cfg(test)]
mod tests {
    use pwf_client::pb;

    use super::decode;

    #[test]
    fn settings_response_requires_the_color_aggregate() {
        let error = decode(pb::GetUserSettingsResponse {
            task_status_colors: None,
        })
        .unwrap_err();

        assert!(error.to_string().contains("without task status colors"));
    }

    #[test]
    fn settings_response_rejects_out_of_range_color_components() {
        let error = decode(pb::GetUserSettingsResponse {
            task_status_colors: Some(pb::TaskStatusColors {
                active: Some(pb::RgbColor {
                    red: 256,
                    green: 0,
                    blue: 0,
                }),
                done: None,
                cancelled: None,
            }),
        })
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("out-of-range active red color component 256")
        );
    }
}
