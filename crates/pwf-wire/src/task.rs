use pwf_models::{
    project::ProjectSourceValue,
    task::{Prerequisites, TaskId, TaskStatus},
};

pub mod session;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrerequisiteStatus {
    pub id: TaskId,
    pub status: Option<TaskStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskView {
    pub id: TaskId,
    pub project: String,
    pub status: TaskStatus,
    pub session: String,
    pub prompt: String,
    pub project_path: ProjectSourceValue,
    pub note: String,
    pub task_file: Option<String>,
    pub line: usize,
    pub format: String,
    pub launchable: bool,
    pub needs_prompt: bool,
    pub issues: Vec<String>,
    pub section: Option<String>,
    pub prerequisites: Option<Prerequisites>,
    pub prerequisite_statuses: Vec<PrerequisiteStatus>,
    pub effort: Option<String>,
    pub tags: Option<String>,
    pub created: Option<String>,
}
