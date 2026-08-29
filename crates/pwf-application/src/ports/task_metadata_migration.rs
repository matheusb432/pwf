use std::{error::Error, path::PathBuf};

use pwf_models::project::{Project, ProjectName};

/// Selects whether task metadata migration reports or persists required changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskMetadataMigrationMode {
    Check,
    Apply,
}

/// Describes one task metadata entry that could not be inspected or changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskMetadataMigrationIssue {
    pub project: ProjectName,
    pub path: Option<PathBuf>,
    pub message: String,
}

impl TaskMetadataMigrationIssue {
    #[must_use]
    pub fn new(project: ProjectName, path: Option<PathBuf>, message: impl Into<String>) -> Self {
        Self {
            project,
            path,
            message: message.into(),
        }
    }
}

/// Summarizes task metadata migration work across managed projects.
#[derive(Debug, Default)]
pub struct TaskMetadataMigrationReport {
    pub project_count: usize,
    pub task_file_count: usize,
    pub task_file_changed_count: usize,
    pub created_at_migrated_count: usize,
    pub completed_at_migrated_count: usize,
    pub completed_at_from_modified_count: usize,
    pub index_entry_changed_count: usize,
    pub issues: Vec<TaskMetadataMigrationIssue>,
}

impl TaskMetadataMigrationReport {
    /// Returns whether applying the migration would change persisted task data.
    #[must_use]
    pub const fn has_changes(&self) -> bool {
        self.task_file_changed_count > 0 || self.index_entry_changed_count > 0
    }

    /// Returns whether any project or file could not be migrated.
    #[must_use]
    pub fn has_issues(&self) -> bool {
        !self.issues.is_empty()
    }

    pub(crate) fn include(&mut self, project: Self) {
        self.task_file_count += project.task_file_count;
        self.task_file_changed_count += project.task_file_changed_count;
        self.created_at_migrated_count += project.created_at_migrated_count;
        self.completed_at_migrated_count += project.completed_at_migrated_count;
        self.completed_at_from_modified_count += project.completed_at_from_modified_count;
        self.index_entry_changed_count += project.index_entry_changed_count;
        self.issues.extend(project.issues);
    }
}

/// Migrates task-note metadata and project-index completion stamps for one project.
pub trait TaskMetadataMigrationClient: Clone + Send + Sync + 'static {
    type Error: Error + Send + Sync + 'static;

    fn migrate_project_task_metadata(
        &self,
        project: &Project,
        mode: TaskMetadataMigrationMode,
    ) -> Result<TaskMetadataMigrationReport, Self::Error>;
}
