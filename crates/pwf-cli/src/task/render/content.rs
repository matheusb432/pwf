use std::fmt::Write as _;

use pwf_client::pb;
use pwf_models::{
    settings::UserSettings,
    task::{Task, TaskTags, TaskTimestamp},
};
use serde::Serialize;

use crate::render::{Field, render_fields};

#[derive(Serialize)]
struct TaskOutput<'a> {
    id: &'a str,
    project: String,
    title: &'a str,
    status: &'a str,
    created: Option<String>,
    completed: Option<String>,
    commits: Option<String>,
    tags: Option<Vec<String>>,
    effort: Option<&'a str>,
    priority: Option<&'a str>,
    blocked_by: Option<Vec<String>>,
    prompt: &'a str,
}

pub(in crate::task) fn json(task: &Task, project: String) -> anyhow::Result<String> {
    let output = TaskOutput {
        id: task.id.as_ref(),
        project,
        title: task.title.as_ref(),
        status: task.status.as_str(),
        created: task.created_at.map(|time| time.date().to_string()),
        completed: task.completed_at.map(|time| time.date().to_string()),
        commits: task.commits.as_ref().map(ToString::to_string),
        tags: task
            .tags
            .as_ref()
            .map(|tags| tags.iter().map(ToString::to_string).collect()),
        effort: task.effort.as_ref().map(AsRef::as_ref),
        priority: task.priority.as_ref().map(AsRef::as_ref),
        blocked_by: task
            .blocked_by
            .as_ref()
            .map(|ids| ids.iter().map(ToString::to_string).collect()),
        prompt: task.prompt.as_ref().trim(),
    };
    serde_json::to_string_pretty(&output).map_err(Into::into)
}

pub(in crate::task) struct TaskContent<'a> {
    id: &'a str,
    title: &'a str,
    status: pb::TaskStatus,
    path: &'a str,
    body: &'a str,
    priority: Option<&'a str>,
    effort: Option<&'a str>,
    tags: Option<&'a str>,
    blocked_by: String,
    commits: Option<&'a str>,
    created_at: Option<&'a str>,
    completed_at: Option<&'a str>,
}

impl<'a> From<&'a pb::TaskRecord> for TaskContent<'a> {
    fn from(task: &'a pb::TaskRecord) -> Self {
        let blocked_by = task
            .blocked_by
            .as_ref()
            .and_then(|value| value.value.as_ref())
            .map_or_else(String::new, |value| match value {
                pb::stored_task_blocked_by::Value::Valid(ids) => ids.values.join(", "),
                pb::stored_task_blocked_by::Value::Malformed(value) => value.raw.clone(),
            });
        Self {
            id: &task.id,
            title: &task.title,
            status: pb::TaskStatus::try_from(task.status).unwrap_or(pb::TaskStatus::Unspecified),
            path: &task.locator,
            body: &task.body,
            priority: task.priority.as_deref(),
            effort: task.effort.as_deref(),
            tags: task.tags.as_deref(),
            blocked_by,
            commits: task.commits.as_deref(),
            created_at: task.created_at.as_deref(),
            completed_at: task.completed_at.as_deref(),
        }
    }
}

impl<'a> From<&'a pb::ListedTask> for TaskContent<'a> {
    fn from(task: &'a pb::ListedTask) -> Self {
        Self {
            id: &task.id,
            title: &task.heading,
            status: pb::TaskStatus::try_from(task.status).unwrap_or(pb::TaskStatus::Unspecified),
            path: &task.file_path,
            body: &task.prompt,
            priority: task.priority.map(priority_name),
            effort: task.effort.map(effort_name),
            tags: task.raw_tags.as_deref(),
            blocked_by: if task.blocked_by_statuses.is_empty() {
                task.blocked_by.join(", ")
            } else {
                super::list::blocked_by_status_summary(&task.blocked_by_statuses)
            },
            commits: task.commits.as_deref(),
            created_at: task.created_at.as_deref(),
            completed_at: task.completed_at.as_deref(),
        }
    }
}

pub(in crate::task) fn rich(
    task: &TaskContent<'_>,
    settings: &UserSettings,
    color: bool,
    columns: Option<usize>,
) -> anyhow::Result<String> {
    let summary = super::render_task_summary(
        task.id,
        task.title,
        task.status,
        false,
        settings.task_status_colors(),
        color,
    );
    let mut fields = vec![Field::new("Path", task.path)];
    for (label, value) in [
        (
            "Priority",
            Some(
                task.priority
                    .unwrap_or(settings.default_priority().as_ref()),
            ),
        ),
        ("Effort", task.effort),
        ("Tags", task.tags),
        ("Commits", task.commits),
    ] {
        if let Some(value) = value {
            fields.push(Field::new(label, value));
        }
    }
    if !task.blocked_by.is_empty() {
        fields.push(Field::new("Blocked by", &task.blocked_by));
    }
    for (label, timestamp) in [
        ("Completed", task.completed_at),
        ("Created", task.created_at),
    ] {
        if let Some(timestamp) = timestamp {
            fields.push(Field::new(
                label,
                settings.datetime_format().format(timestamp.parse()?)?,
            ));
        }
    }
    let mut output = format!(
        "{summary}\n\n{}\n\n",
        render_fields(&fields, color, columns)
    );
    output.push_str(task.body.trim_start_matches(['\r', '\n']));
    Ok(output)
}

pub(in crate::task) fn rich_list(
    result: &pb::ListTasksResponse,
    settings: &UserSettings,
    color: bool,
    columns: Option<usize>,
) -> anyhow::Result<String> {
    if result.tasks.is_empty() {
        return Ok(super::render_list(
            result,
            result
                .project_task_path
                .as_deref()
                .unwrap_or("managed project task paths"),
            settings.task_status_colors(),
            color,
        ));
    }
    let mut output = String::new();
    for task in &result.tasks {
        if !output.is_empty() {
            if !output.ends_with('\n') {
                output.push('\n');
            }
            output.push('\n');
        }
        output.push_str(&rich(&TaskContent::from(task), settings, color, columns)?);
        let diagnostics = super::list::diagnostics(task);
        if !diagnostics.is_empty() {
            if !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(&diagnostics);
        }
    }
    if result.hidden > 0 {
        let _ = write!(
            output,
            "\n... and {} more; use '--all' to list everything",
            result.hidden
        );
    }
    Ok(output)
}

pub(in crate::task) fn json_list(tasks: &[pb::ListedTask]) -> anyhow::Result<String> {
    let output = tasks
        .iter()
        .map(|task| {
            if let Some(issue) = task.blocked_by_issues.first() {
                anyhow::bail!(
                    "Malformed blocked_by metadata {:?} in {}: {}",
                    issue.raw,
                    issue.path,
                    issue.reason
                );
            }
            let tags = task
                .raw_tags
                .as_deref()
                .map(TaskTags::parse_frontmatter)
                .transpose()?;
            Ok(TaskOutput {
                id: &task.id,
                project: task.project.clone(),
                title: &task.heading,
                status: status_name(task.status),
                created: task
                    .created_at
                    .as_deref()
                    .map(str::parse::<TaskTimestamp>)
                    .transpose()?
                    .map(|time| time.date().to_string()),
                completed: task
                    .completed_at
                    .as_deref()
                    .map(str::parse::<TaskTimestamp>)
                    .transpose()?
                    .map(|time| time.date().to_string()),
                commits: task
                    .commits
                    .as_deref()
                    .map(|value| {
                        pwf_models::task::CommitRanges::try_new(value.trim_matches(['\'', '"']))
                    })
                    .transpose()?
                    .map(|value| value.to_string()),
                tags: tags.map(|tags| tags.iter().map(ToString::to_string).collect()),
                effort: task.effort.map(effort_name),
                priority: task.priority.map(priority_name),
                blocked_by: if task.blocked_by.is_empty() {
                    None
                } else {
                    Some(task.blocked_by.clone())
                },
                prompt: task.prompt.trim(),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    serde_json::to_string_pretty(&output).map_err(Into::into)
}

fn priority_name(value: i32) -> &'static str {
    match pb::PriorityTier::try_from(value).ok() {
        Some(pb::PriorityTier::Low) => "low",
        Some(pb::PriorityTier::Medium) => "medium",
        Some(pb::PriorityTier::High) => "high",
        Some(pb::PriorityTier::Highest) => "highest",
        Some(pb::PriorityTier::Unspecified) | None => "unspecified",
    }
}

fn effort_name(value: i32) -> &'static str {
    match pb::EffortTier::try_from(value).ok() {
        Some(pb::EffortTier::Low) => "low",
        Some(pb::EffortTier::Medium) => "medium",
        Some(pb::EffortTier::High) => "high",
        Some(pb::EffortTier::Highest) => "highest",
        Some(pb::EffortTier::Unspecified) | None => "unspecified",
    }
}

fn status_name(value: i32) -> &'static str {
    match pb::TaskStatus::try_from(value).ok() {
        Some(pb::TaskStatus::Active) => "active",
        Some(pb::TaskStatus::Done) => "done",
        Some(pb::TaskStatus::Cancelled) => "cancelled",
        Some(pb::TaskStatus::Backlog) => "backlog",
        Some(pb::TaskStatus::Unspecified) | None => "unspecified",
    }
}

#[cfg(test)]
mod tests {
    use pwf_models::{
        revision::ContentRevision,
        task::{BlockedBy, CommitRanges, TaskId, TaskPrompt, TaskStatus, TaskTags, TaskTitle},
    };

    use super::*;

    #[test]
    fn json_preserves_the_public_projection() {
        let task = Task {
            id: TaskId::try_new("FOO-0001").unwrap(),
            title: TaskTitle::try_new("Typed task").unwrap(),
            status: TaskStatus::Done,
            prompt: TaskPrompt::new("\n  authored body  \n"),
            created_at: Some("2026-07-26T12:34:56Z".parse().unwrap()),
            completed_at: Some("2026-08-12T12:34:56Z".parse().unwrap()),
            commits: Some(CommitRanges::try_new("a..b, c..d").unwrap()),
            tags: Some(TaskTags::parse_frontmatter("[rust, sqlite]").unwrap()),
            effort: Some("high".parse().unwrap()),
            priority: Some("highest".parse().unwrap()),
            blocked_by: Some(BlockedBy::try_new(["AUX-0014".parse().unwrap()]).unwrap()),
            revision: ContentRevision::try_new("a".repeat(64)).unwrap(),
        };
        let value: serde_json::Value =
            serde_json::from_str(&json(&task, "foo".to_string()).unwrap()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "id": "FOO-0001", "project": "foo", "title": "Typed task", "status": "done",
                "created": "2026-07-26", "completed": "2026-08-12", "commits": "a..b, c..d",
                "tags": ["rust", "sqlite"], "effort": "high", "priority": "highest",
                "blocked_by": ["AUX-0014"], "prompt": "authored body"
            })
        );
    }
}
