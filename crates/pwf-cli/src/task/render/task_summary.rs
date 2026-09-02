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
    render_task_identifier_with_status(identifier, LifecycleStatus::from(status), color_on)
}

pub(in crate::task) fn render_domain_task_identifier(
    identifier: &str,
    status: pwf_models::task::TaskStatus,
    color_on: bool,
) -> String {
    render_task_identifier_with_status(identifier, LifecycleStatus::from(status), color_on)
}

fn render_task_identifier_with_status(
    identifier: &str,
    status: LifecycleStatus,
    color_on: bool,
) -> String {
    if !color_on {
        return identifier.to_string();
    }
    paint(identifier, lifecycle_status_color(status), true)
}

fn task_status_color(status: TaskStatus) -> anstyle::Color {
    lifecycle_status_color(status.into())
}

fn lifecycle_status_color(status: LifecycleStatus) -> anstyle::Color {
    match status {
        LifecycleStatus::Active => AnsiColor::Blue.into(),
        LifecycleStatus::Done => AnsiColor::Green.into(),
        LifecycleStatus::Cancelled => AnsiColor::Red.into(),
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
