use anstyle::AnsiColor;
use pwf_client::pb::TaskStatus;

use crate::render::{paint, render_summary};

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
    let identifier = render_task_identifier(identifier, status, color_on);
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
    paint(text, task_status_color(status), true)
}

pub(in crate::task) fn render_task_identifier(
    identifier: &str,
    status: TaskStatus,
    color_on: bool,
) -> String {
    if !color_on {
        return identifier.to_string();
    }
    paint(identifier, task_status_color(status), true)
}

fn task_status_color(status: TaskStatus) -> anstyle::Color {
    match status {
        TaskStatus::Active => AnsiColor::Blue.into(),
        TaskStatus::Done => AnsiColor::Green.into(),
        TaskStatus::Cancelled => AnsiColor::Red.into(),
        TaskStatus::Unspecified => AnsiColor::Yellow.into(),
    }
}
