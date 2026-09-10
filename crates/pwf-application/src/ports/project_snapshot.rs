use pwf_models::{note::ProjectNote, project::Project};

use super::task_vault::TaskSummaryRecord;

pub trait ProjectSnapshotWriter: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn write_project_snapshot(
        &self,
        project: &Project,
        tasks: &[TaskSummaryRecord],
        notes: &[ProjectNote],
    ) -> Result<(), Self::Error>;
}
