use pwf_models::project::Project;

use crate::ports::{
    project_note::ProjectNotes, project_snapshot::ProjectSnapshotWriter, task_vault::TaskVault,
};

#[derive(Debug, thiserror::Error)]
pub enum RefreshProjectSnapshotError {
    #[error("reading snapshot tasks: {0}")]
    ReadTasks(#[source] anyhow::Error),
    #[error("reading snapshot notes: {0}")]
    ReadNotes(#[source] anyhow::Error),
    #[error("writing project snapshot: {0}")]
    WriteSnapshot(#[source] anyhow::Error),
}

#[cqrsy::command]
pub fn execute(
    project: &Project,
    tasks: &impl TaskVault,
    notes: &impl ProjectNotes,
    writer: &impl ProjectSnapshotWriter,
) -> Result<(), RefreshProjectSnapshotError> {
    if !project.snapshot_enabled {
        return Ok(());
    }
    let tasks = tasks
        .list_task_summaries(project)
        .map_err(|error| RefreshProjectSnapshotError::ReadTasks(anyhow::Error::new(error)))?;
    let notes = notes
        .list_notes(project)
        .map_err(|error| RefreshProjectSnapshotError::ReadNotes(anyhow::Error::new(error)))?;
    writer
        .write_project_snapshot(project, &tasks, &notes)
        .map_err(|error| RefreshProjectSnapshotError::WriteSnapshot(anyhow::Error::new(error)))
}
