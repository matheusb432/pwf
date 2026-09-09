use anstyle::{AnsiColor, Color};
use pwf_client::pb;
use pwf_models::{settings::TaskStatusColors, task::TaskStatus};

use crate::render::{paint, rgb_color};

pub(in crate::task) fn render_task_summary(
    identifier: &str,
    title: &str,
    status: pb::TaskStatus,
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
    status: pb::TaskStatus,
    task_status_colors: TaskStatusColors,
    color_on: bool,
) -> String {
    let text = match status {
        pb::TaskStatus::Active => "active",
        pb::TaskStatus::Done => "done",
        pb::TaskStatus::Cancelled => "cancelled",
        pb::TaskStatus::Unspecified => "unspecified",
    };
    if !color_on {
        return text.to_string();
    }
    paint(text, task_status_color(status, task_status_colors), true)
}

pub(in crate::task) fn render_task_identifier(
    identifier: &str,
    status: pb::TaskStatus,
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

fn task_status_color(status: pb::TaskStatus, task_status_colors: TaskStatusColors) -> Color {
    lifecycle_status_color(status.into(), task_status_colors)
}

fn lifecycle_status_color(status: LifecycleStatus, task_status_colors: TaskStatusColors) -> Color {
    match status {
        LifecycleStatus::Active => rgb_color(task_status_colors.active()).into(),
        LifecycleStatus::Done => rgb_color(task_status_colors.done()).into(),
        LifecycleStatus::Cancelled => rgb_color(task_status_colors.cancelled()).into(),
        LifecycleStatus::Unspecified => AnsiColor::Yellow.into(),
    }
}

#[derive(Clone, Copy)]
enum LifecycleStatus {
    Active,
    Done,
    Cancelled,
    Unspecified,
}

impl From<pb::TaskStatus> for LifecycleStatus {
    fn from(status: pb::TaskStatus) -> Self {
        match status {
            pb::TaskStatus::Active => Self::Active,
            pb::TaskStatus::Done => Self::Done,
            pb::TaskStatus::Cancelled => Self::Cancelled,
            pb::TaskStatus::Unspecified => Self::Unspecified,
        }
    }
}

impl From<TaskStatus> for LifecycleStatus {
    fn from(status: TaskStatus) -> Self {
        match status {
            TaskStatus::Active => Self::Active,
            TaskStatus::Done => Self::Done,
            TaskStatus::Cancelled => Self::Cancelled,
        }
    }
}
