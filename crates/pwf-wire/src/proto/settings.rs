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
    pb::GetUserSettingsResponse {
        default_priority: match settings.default_priority() {
            PriorityTier::Low => pb::PriorityTier::Low,
            PriorityTier::Medium => pb::PriorityTier::Medium,
            PriorityTier::High => pb::PriorityTier::High,
            PriorityTier::Highest => pb::PriorityTier::Highest,
        } as i32,
        default_sort_order: Some(order_spec(settings.default_sort_order())),
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
