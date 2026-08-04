use pwf_application::task::show_task::TaskData;
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
    prerequisites: Option<Vec<String>>,
    section: Option<String>,
    prompt: String,
}

pub(super) fn json(task: TaskData) -> Result<String, String> {
    serde_json::to_string_pretty(&TaskOutput::from(task))
        .map_err(|error| format!("rendering task JSON failed: {error}"))
}

impl From<TaskData> for TaskOutput {
    fn from(task: TaskData) -> Self {
        Self {
            id: task.id.to_string(),
            project: task.project.to_string(),
            title: task.title,
            status: task.status.to_string(),
            created: task.created.map(|value| value.as_str().to_string()),
            completed: task.completed.map(|value| value.as_str().to_string()),
            commits: task.commits,
            tags: task
                .tags
                .map(|tags| tags.iter().map(|tag| tag.as_ref().to_string()).collect()),
            effort: task.effort.map(|effort| effort.to_string()),
            prerequisites: task
                .prerequisites
                .map(|identifiers| identifiers.into_iter().map(|id| id.to_string()).collect()),
            section: task.section,
            prompt: task.prompt,
        }
    }
}
