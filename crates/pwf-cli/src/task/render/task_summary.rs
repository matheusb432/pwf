use anstyle::AnsiColor;
use pwf_models::task::TaskStatus;

use super::{ID_ORANGE, paint};

pub(in crate::task) fn render_task_summary(
    identifier: &str,
    title: &str,
    status: Option<TaskStatus>,
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
        Some(status) => {
            format!(
                "{identifier} [{}] :: {title}",
                render_status(status, color_on)
            )
        }
    }
}

pub(in crate::task) fn render_status(status: TaskStatus, color_on: bool) -> String {
    let text = status.to_string();
    if !color_on {
        return text;
    }
    let color: anstyle::Color = match status {
        TaskStatus::Active => ID_ORANGE.into(),
        TaskStatus::Done => AnsiColor::Green.into(),
        TaskStatus::Cancelled => AnsiColor::Red.into(),
    };
    paint(&text, color, true)
}
