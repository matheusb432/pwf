//! Loads validated presentation settings through the resident server.

use anyhow::Context as _;
use pwf_client::{pb, settings::SettingsClient};
use pwf_models::{
    settings::{NoteStatusColors, ProjectStatusColors, RgbColor, TaskStatusColors, UserSettings},
    task::{
        PriorityTier,
        order::{OrderDirection, OrderField, OrderSpec},
    },
};

/// Loads the latest complete user-settings snapshot.
pub async fn load(client: &SettingsClient) -> anyhow::Result<UserSettings> {
    let response = client.get_user_settings().await.map_err(crate::rpc_error)?;
    decode(response)
}

fn decode(response: pb::GetUserSettingsResponse) -> anyhow::Result<UserSettings> {
    let colors = response
        .task_status_colors
        .context("pwf-server returned user settings without task status colors")?;
    let project_colors = response
        .project_status_colors
        .context("pwf-server returned user settings without project status colors")?;
    let note_colors = response
        .note_status_colors
        .context("pwf-server returned user settings without note status colors")?;
    let default_priority = match pb::PriorityTier::try_from(response.default_priority).ok() {
        Some(pb::PriorityTier::Low) => PriorityTier::Low,
        Some(pb::PriorityTier::Medium) => PriorityTier::Medium,
        Some(pb::PriorityTier::High) => PriorityTier::High,
        Some(pb::PriorityTier::Highest) => PriorityTier::Highest,
        _ => anyhow::bail!("pwf-server returned invalid default priority"),
    };
    let default_sort_order = decode_order(
        response
            .default_sort_order
            .context("pwf-server returned user settings without default sort order")?,
    )?;
    Ok(UserSettings::new(
        TaskStatusColors::new(
            Some(decode_color("active", colors.active)?),
            Some(decode_color("done", colors.done)?),
            Some(decode_color("cancelled", colors.cancelled)?),
        ),
        ProjectStatusColors::new(
            Some(decode_color("project.active", project_colors.active)?),
            Some(decode_color("project.paused", project_colors.paused)?),
        ),
        NoteStatusColors::new(
            Some(decode_color("note.active", note_colors.active)?),
            Some(decode_color("note.verified", note_colors.verified)?),
        ),
        default_priority,
        default_sort_order,
    ))
}

fn decode_order(order: pb::OrderSpec) -> anyhow::Result<OrderSpec> {
    let field = match pb::OrderField::try_from(order.field).ok() {
        Some(pb::OrderField::Created) => OrderField::Created,
        Some(pb::OrderField::Id) => OrderField::Id,
        Some(pb::OrderField::ProjectId) => OrderField::ProjectId,
        Some(pb::OrderField::Priority) => OrderField::Priority,
        Some(pb::OrderField::Effort) => OrderField::Effort,
        Some(pb::OrderField::Title) => OrderField::Title,
        _ => anyhow::bail!("pwf-server returned invalid default sort field"),
    };
    let direction = match pb::OrderDirection::try_from(order.direction).ok() {
        Some(pb::OrderDirection::Asc) => OrderDirection::Asc,
        Some(pb::OrderDirection::Desc) => OrderDirection::Desc,
        _ => anyhow::bail!("pwf-server returned invalid default sort direction"),
    };
    Ok(OrderSpec { field, direction })
}

fn decode_color(field: &'static str, color: Option<pb::RgbColor>) -> anyhow::Result<RgbColor> {
    let color = color
        .with_context(|| format!("pwf-server returned user settings without {field} color"))?;
    Ok(RgbColor::new(
        decode_component(field, "red", color.red)?,
        decode_component(field, "green", color.green)?,
        decode_component(field, "blue", color.blue)?,
    ))
}

fn decode_component(field: &str, component: &str, value: u32) -> anyhow::Result<u8> {
    u8::try_from(value).with_context(|| {
        format!("pwf-server returned out-of-range {field} {component} color component {value}")
    })
}

#[cfg(test)]
mod tests {
    use pwf_client::pb;

    use super::{UserSettings, decode};

    fn valid_response() -> pb::GetUserSettingsResponse {
        pb::GetUserSettingsResponse {
            task_status_colors: Some(pb::TaskStatusColors {
                active: Some(pb::RgbColor {
                    red: 100,
                    green: 149,
                    blue: 237,
                }),
                done: Some(pb::RgbColor {
                    red: 163,
                    green: 230,
                    blue: 53,
                }),
                cancelled: Some(pb::RgbColor {
                    red: 255,
                    green: 107,
                    blue: 138,
                }),
            }),
            project_status_colors: Some(pb::ProjectStatusColors {
                active: Some(pb::RgbColor {
                    red: 100,
                    green: 149,
                    blue: 237,
                }),
                paused: Some(pb::RgbColor {
                    red: 255,
                    green: 107,
                    blue: 138,
                }),
            }),
            note_status_colors: Some(pb::NoteStatusColors {
                active: Some(pb::RgbColor {
                    red: 100,
                    green: 149,
                    blue: 237,
                }),
                verified: Some(pb::RgbColor {
                    red: 163,
                    green: 230,
                    blue: 53,
                }),
            }),
            default_priority: pb::PriorityTier::Medium as i32,
            default_sort_order: Some(pb::OrderSpec {
                field: pb::OrderField::Id as i32,
                direction: pb::OrderDirection::Desc as i32,
            }),
        }
    }

    #[test]
    fn settings_response_validates_list_defaults() {
        assert_eq!(decode(valid_response()).unwrap(), UserSettings::default());
        let mut response = valid_response();
        response.default_priority = 999;
        assert!(decode(response).is_err());
        let mut response = valid_response();
        response.default_sort_order = None;
        assert!(decode(response).is_err());
        for order in [
            pb::OrderSpec {
                field: 999,
                direction: 1,
            },
            pb::OrderSpec {
                field: 1,
                direction: 999,
            },
        ] {
            let mut response = valid_response();
            response.default_sort_order = Some(order);
            assert!(decode(response).is_err());
        }
    }

    #[test]
    fn settings_response_requires_the_color_aggregate() {
        let error = decode(pb::GetUserSettingsResponse {
            task_status_colors: None,
            ..valid_response()
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
            ..valid_response()
        })
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("out-of-range active red color component 256")
        );
    }
}
