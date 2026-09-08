//! Process-neutral confirmation contracts shared by PWF frontends.

use pwf_models::{
    AppDate,
    note::{NoteId, NoteTitle},
    project::ProjectName,
    revision::ContentRevision,
    task::{TaskId, TaskStatus, TaskTitle},
};

use super::task::TaskNotePath;

/// Identifies the project note deleted after confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveNoteConfirmation {
    pub note_identifier: NoteId,
    pub project: ProjectName,
    pub title: NoteTitle,
}

/// Identifies the task and note deleted after confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveTaskConfirmation {
    pub task_identifier: TaskId,
    pub project: ProjectName,
    pub title: TaskTitle,
    pub status: TaskStatus,
    pub note_path: TaskNotePath,
    pub deletion: TaskDeletion,
    pub revision: ContentRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskDeletion {
    HardDelete,
    MoveToTrash { obsidian_vault: std::path::PathBuf },
}

impl TaskDeletion {
    #[must_use]
    pub fn trash_folder(&self) -> Option<std::path::PathBuf> {
        match self {
            Self::HardDelete => None,
            Self::MoveToTrash { obsidian_vault } => Some(obsidian_vault.join(".trash")),
        }
    }
}

/// Identifies the completion data discarded after confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReopenTaskConfirmation {
    pub task_identifier: TaskId,
    pub project: ProjectName,
    pub completion_date: Option<AppDate>,
    pub commit_provenance: Option<String>,
    pub report: Option<String>,
    pub revision: ContentRevision,
}
