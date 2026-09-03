use anstyle::AnsiColor;
use pwf_client::pb::TaskStatus;
use pwf_models::settings::TaskStatusColors;

use crate::render::paint;

pub(in crate::task) fn render_task_summary(
    identifier: &str,
    title: &str,
    status: TaskStatus,
    status_visible: bool,
    task_status_colors: TaskStatusColors,
    color_on: bool,
) -> String {
    // Plain output is a raw-text contract; Markdown emphasis is reserved for ANSI rendering.
    let identifier = render_task_identifier(identifier, status, task_status_colors, color_on);
    if !status_visible {
        return format!("{identifier} :: {title}");
    }
    format!(
        "{identifier} [{}] :: {title}",
        render_status(status, task_status_colors, color_on)
    )
}

pub(in crate::task) fn render_status(
    status: TaskStatus,
    task_status_colors: TaskStatusColors,
    color_on: bool,
) -> String {
    let text = match status {
        TaskStatus::Active => "active",
        TaskStatus::Done => "done",
        TaskStatus::Cancelled => "cancelled",
        TaskStatus::Unspecified => "unspecified",
    };
    if !color_on {
        return text.to_string();
    }
    paint(text, task_status_color(status, task_status_colors), true)
}

pub(in crate::task) fn render_task_identifier(
    identifier: &str,
    status: TaskStatus,
    task_status_colors: TaskStatusColors,
    color_on: bool,
) -> String {
    render_task_identifier_with_status(
        identifier,
        LifecycleStatus::from(status),
        task_status_colors,
        color_on,
    )
}

pub(in crate::task) fn render_domain_task_identifier(
    identifier: &str,
    status: pwf_models::task::TaskStatus,
    task_status_colors: TaskStatusColors,
    color_on: bool,
) -> String {
    render_task_identifier_with_status(
        identifier,
        LifecycleStatus::from(status),
        task_status_colors,
        color_on,
    )
}

fn render_task_identifier_with_status(
    identifier: &str,
    status: LifecycleStatus,
    task_status_colors: TaskStatusColors,
    color_on: bool,
) -> String {
    if !color_on {
        return identifier.to_string();
    }
    paint(
        identifier,
        lifecycle_status_color(status, task_status_colors),
        true,
    )
}

fn task_status_color(status: TaskStatus, task_status_colors: TaskStatusColors) -> anstyle::Color {
    lifecycle_status_color(status.into(), task_status_colors)
}

fn lifecycle_status_color(
    status: LifecycleStatus,
    task_status_colors: TaskStatusColors,
) -> anstyle::Color {
    let configured = match status {
        LifecycleStatus::Active => task_status_colors.active(),
        LifecycleStatus::Done => task_status_colors.done(),
        LifecycleStatus::Cancelled => task_status_colors.cancelled(),
        LifecycleStatus::Unspecified => None,
    };
    if let Some(color) = configured {
        return anstyle::RgbColor(color.red(), color.green(), color.blue()).into();
    }
    match status {
        LifecycleStatus::Active => AnsiColor::Blue,
        LifecycleStatus::Done => AnsiColor::Green,
        LifecycleStatus::Cancelled => AnsiColor::Red,
        LifecycleStatus::Unspecified => AnsiColor::Yellow,
    }
    .into()
}

#[derive(Clone, Copy)]
enum LifecycleStatus {
    Active,
    Done,
    Cancelled,
    Unspecified,
}

impl From<TaskStatus> for LifecycleStatus {
    fn from(status: TaskStatus) -> Self {
        match status {
            TaskStatus::Active => Self::Active,
            TaskStatus::Done => Self::Done,
            TaskStatus::Cancelled => Self::Cancelled,
            TaskStatus::Unspecified => Self::Unspecified,
        }
    }
}

impl From<pwf_models::task::TaskStatus> for LifecycleStatus {
    fn from(status: pwf_models::task::TaskStatus) -> Self {
        match status {
            pwf_models::task::TaskStatus::Active => Self::Active,
            pwf_models::task::TaskStatus::Done => Self::Done,
            pwf_models::task::TaskStatus::Cancelled => Self::Cancelled,
        }
    }
}
