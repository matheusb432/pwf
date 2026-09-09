//! Explicit protobuf mappings for user-settings operations.

use pwf_models::{
    settings::{RgbColor, UserSettings},
    task::{
        PriorityTier,
        order::{OrderDirection, OrderField, OrderSpec},
    },
};

use crate::pb;

#[must_use]
pub fn get_user_settings_response(settings: UserSettings) -> pb::GetUserSettingsResponse {
    let colors = settings.task_status_colors();
    let project_colors = settings.project_status_colors();
    let note_colors = settings.note_status_colors();
    pb::GetUserSettingsResponse {
        project_status_colors: Some(pb::ProjectStatusColors {
            active: Some(rgb_color(project_colors.active())),
            paused: Some(rgb_color(project_colors.paused())),
        }),
        note_status_colors: Some(pb::NoteStatusColors {
            active: Some(rgb_color(note_colors.active())),
            verified: Some(rgb_color(note_colors.verified())),
        }),
        default_priority: match settings.default_priority() {
            PriorityTier::Low => pb::PriorityTier::Low,
            PriorityTier::Medium => pb::PriorityTier::Medium,
            PriorityTier::High => pb::PriorityTier::High,
            PriorityTier::Highest => pb::PriorityTier::Highest,
        } as i32,
        default_sort_order: Some(order_spec(settings.default_sort_order())),
        task_status_colors: Some(pb::TaskStatusColors {
            active: Some(rgb_color(colors.active())),
            done: Some(rgb_color(colors.done())),
            cancelled: Some(rgb_color(colors.cancelled())),
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

fn order_spec(order: OrderSpec) -> pb::OrderSpec {
    pb::OrderSpec {
        field: match order.field {
            OrderField::Created => pb::OrderField::Created,
            OrderField::Id => pb::OrderField::Id,
            OrderField::ProjectId => pb::OrderField::ProjectId,
            OrderField::Priority => pb::OrderField::Priority,
            OrderField::Effort => pb::OrderField::Effort,
            OrderField::Title => pb::OrderField::Title,
        } as i32,
        direction: match order.direction {
            OrderDirection::Asc => pb::OrderDirection::Asc,
            OrderDirection::Desc => pb::OrderDirection::Desc,
        } as i32,
    }
}
