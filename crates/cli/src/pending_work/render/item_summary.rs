use anstyle::AnsiColor;
use pwf_models::pending_work::WorkItemStatus;

use super::{ID_ORANGE, paint};

#[derive(Clone, Copy)]
pub(in crate::pending_work) enum StatusPlacement {
    AfterIdentifier,
    AfterTitle,
}

pub(in crate::pending_work) fn render_item_summary(
    identifier: &str,
    title: &str,
    status: Option<(WorkItemStatus, StatusPlacement)>,
    color_on: bool,
) -> String {
    // Plain output is a raw-text contract; Markdown emphasis is reserved for ANSI rendering.
    let identifier = if color_on {
        paint(identifier, ID_ORANGE, true)
    } else {
        identifier.to_string()
    };
    match status {
        None => format!("{identifier} :: {title}"),
        Some((status, StatusPlacement::AfterIdentifier)) => {
            format!(
                "{identifier} [{}] :: {title}",
                render_status(status, color_on)
            )
        }
        Some((status, StatusPlacement::AfterTitle)) => {
            format!(
                "{identifier} :: {title} ({})",
                render_status(status, color_on)
            )
        }
    }
}

pub(in crate::pending_work) fn render_status(status: WorkItemStatus, color_on: bool) -> String {
    let text = status.to_string();
    if !color_on {
        return text;
    }
    let color: anstyle::Color = match status {
        WorkItemStatus::Active => ID_ORANGE.into(),
        WorkItemStatus::Done => AnsiColor::Green.into(),
        WorkItemStatus::Cancelled => AnsiColor::Red.into(),
    };
    paint(&text, color, true)
}
