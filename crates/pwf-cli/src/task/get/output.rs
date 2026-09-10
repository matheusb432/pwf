use pwf_models::task::Task;
use serde::Serialize;

#[derive(Serialize)]
struct TaskOutput<'a> {
    id: &'a str,
    project: String,
    title: &'a str,
    status: &'a str,
    created: Option<String>,
    completed: Option<String>,
    commits: Option<&'a str>,
    tags: Option<Vec<&'a str>>,
    effort: Option<&'a str>,
    priority: Option<&'a str>,
    blocked_by: Option<Vec<&'a str>>,
    prompt: &'a str,
}

pub(super) fn json(task: &Task, project: String) -> anyhow::Result<String> {
    let output = TaskOutput {
        id: task.id.as_ref(),
        project,
        title: task.title.as_ref(),
        status: task.status.as_str(),
        created: task.created_at.map(|time| time.date().to_string()),
        completed: task.completed_at.map(|time| time.date().to_string()),
        commits: task.commits.as_ref().map(AsRef::as_ref),
        tags: task
            .tags
            .as_ref()
            .map(|tags| tags.iter().map(AsRef::as_ref).collect()),
        effort: task.effort.as_ref().map(AsRef::as_ref),
        priority: task.priority.as_ref().map(AsRef::as_ref),
        blocked_by: task
            .blocked_by
            .as_ref()
            .map(|ids| ids.iter().map(AsRef::as_ref).collect()),
        prompt: task.prompt.as_ref().trim(),
    };
    serde_json::to_string_pretty(&output).map_err(Into::into)
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
                "id": "FOO-0001", "project": "foo", "title": "typed task", "status": "done",
                "created": "2026-07-26", "completed": "2026-08-12", "commits": "a..b, c..d",
                "tags": ["rust", "sqlite"], "effort": "high", "priority": "highest",
                "blocked_by": ["AUX-0014"], "prompt": "authored body"
            })
        );
    }
}
