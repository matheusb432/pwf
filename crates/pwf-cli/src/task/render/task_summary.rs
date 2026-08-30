use anstyle::AnsiColor;
use pwf_client::pb::TaskStatus;

use crate::render::{ID_ORANGE, paint, render_summary};

pub(in crate::task) fn render_task_summary(
    identifier: &str,
    title: &str,
    status: Option<TaskStatus>,
    color_on: bool,
) -> String {
    let Some(status) = status else {
        return render_summary(identifier, title, color_on);
    };
    // Plain output is a raw-text contract; Markdown emphasis is reserved for ANSI rendering.
    let identifier = if color_on {
        paint(identifier, ID_ORANGE, true)
    } else {
        identifier.to_string()
    };
    format!(
        "{identifier} [{}] :: {title}",
        render_status(status, color_on)
    )
}

pub(in crate::task) fn render_status(status: TaskStatus, color_on: bool) -> String {
    let text = match status {
        TaskStatus::Active => "active",
        TaskStatus::Done => "done",
        TaskStatus::Cancelled => "cancelled",
        TaskStatus::Unspecified => "unspecified",
    };
    if !color_on {
        return text.to_string();
    }
    let color: anstyle::Color = match status {
        TaskStatus::Active => ID_ORANGE.into(),
        TaskStatus::Done => AnsiColor::Green.into(),
        TaskStatus::Cancelled => AnsiColor::Red.into(),
        TaskStatus::Unspecified => AnsiColor::Yellow.into(),
    };
    paint(text, color, true)
}
