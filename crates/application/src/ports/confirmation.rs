use std::path::PathBuf;

use pwf_models::pending_work::{ProjectName, WorkItemId, WorkItemStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirmation {
    Removal {
        pending_work_identifier: WorkItemId,
        project: ProjectName,
        title: String,
        status: WorkItemStatus,
        note_path: PathBuf,
    },
}

pub trait ConfirmationClient {
    fn confirm(&self, confirmation: &Confirmation) -> bool;
}
