use pwf_wire::task::TaskData;
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
            id: task.id.to_string(),
            project: task.project.to_string(),
            title: task.title.to_string(),
            status: task.status.to_string(),
            created: task.created.map(|value| value.to_string()),
            completed: task.completed.map(|value| value.to_string()),
            commits: task.commits.map(|commits| commits.to_string()),
            tags: task
                .tags
                .map(|tags| tags.iter().map(|tag| tag.as_ref().to_string()).collect()),
            effort: task.effort.map(|effort| effort.to_string()),
            blocked_by: task
                .blocked_by
                .map(|identifiers| identifiers.iter().map(ToString::to_string).collect()),
            section: task.section.map(|section| section.to_string()),
            prompt: task.prompt.to_string(),
        }
    }
}
