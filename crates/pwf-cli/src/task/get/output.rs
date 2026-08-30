use pwf_client::pb::{EffortTier, PriorityTier, TaskData, TaskStatus};
use serde::Serialize;

#[derive(Serialize)]
struct TaskOutput {
    id: String,
    project: String,
    title: String,
    status: String,
    created: Option<String>,
    completed: Option<String>,
    commits: Option<String>,
    tags: Option<Vec<String>>,
    effort: Option<String>,
    priority: Option<String>,
    blocked_by: Option<Vec<String>>,
    section: Option<String>,
    prompt: String,
}

pub(super) fn json(task: TaskData) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&TaskOutput::from(task))
}

impl From<TaskData> for TaskOutput {
    fn from(task: TaskData) -> Self {
        Self {
            id: task.id,
            project: task.project,
            title: task.title,
            status: task_status(task.status).to_string(),
            created: task.created,
            completed: task.completed,
            commits: task.commits,
            tags: (!task.tags.is_empty()).then_some(task.tags),
            effort: task
                .effort
                .and_then(|effort| effort_name(effort).map(str::to_string)),
            priority: task
                .priority
                .and_then(|priority| priority_name(priority).map(str::to_string)),
            blocked_by: (!task.blocked_by.is_empty()).then_some(task.blocked_by),
            section: task.section,
            prompt: task.prompt,
        }
    }
}

fn task_status(value: i32) -> &'static str {
    match TaskStatus::try_from(value).ok() {
        Some(TaskStatus::Active) => "active",
        Some(TaskStatus::Done) => "done",
        Some(TaskStatus::Cancelled) => "cancelled",
        Some(TaskStatus::Unspecified) | None => "unspecified",
    }
}

fn effort_name(value: i32) -> Option<&'static str> {
    match EffortTier::try_from(value).ok()? {
        EffortTier::Low => Some("low"),
        EffortTier::Medium => Some("medium"),
        EffortTier::High => Some("high"),
        EffortTier::Highest => Some("highest"),
        EffortTier::Unspecified => None,
    }
}

fn priority_name(value: i32) -> Option<&'static str> {
    match PriorityTier::try_from(value).ok()? {
        PriorityTier::Low => Some("low"),
        PriorityTier::Medium => Some("medium"),
        PriorityTier::High => Some("high"),
        PriorityTier::Highest => Some("highest"),
        PriorityTier::Unspecified => None,
    }
}
