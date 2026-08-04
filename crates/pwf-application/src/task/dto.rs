use pwf_models::{
    project::{Project, ProjectSourceValue},
    task::{Prerequisites, TaskId, TaskStatus},
};

use crate::ports::task_record::TaskPatch;

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

pub(in crate::task) struct TaskIdentity {
    pub(in crate::task) project: Project,
    pub(in crate::task) identifier: TaskId,
}

pub(in crate::task) struct PreparedTaskUpdate {
    pub(in crate::task) identity: TaskIdentity,
    pub(in crate::task) patch: TaskPatch,
    pub(in crate::task) outcome: super::update_task::UpdateTaskOk,
}
