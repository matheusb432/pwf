use pwf_models::{
    project::ProjectName,
    task::{TaskId, TaskStatus, TaskTitle},
};
use pwf_wire::task::TaskNotePath;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirmation {
    Removal {
        task_identifier: TaskId,
        project: ProjectName,
        title: TaskTitle,
        status: TaskStatus,
        note_path: TaskNotePath,
    },
}

pub trait ConfirmationClient {
    fn confirm(&self, confirmation: &Confirmation) -> bool;
}
