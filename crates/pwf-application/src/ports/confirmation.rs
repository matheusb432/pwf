use std::path::PathBuf;

use pwf_models::task::{ProjectName, TaskId, TaskStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirmation {
    Removal {
        task_identifier: TaskId,
        project: ProjectName,
        title: String,
        status: TaskStatus,
        note_path: PathBuf,
    },
}

pub trait ConfirmationClient {
    fn confirm(&self, confirmation: &Confirmation) -> bool;
}
