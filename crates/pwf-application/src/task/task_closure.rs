//! Applies the shared task completion and cancellation transition.

use pwf_models::{
    project::ProjectId,
    task::{CommitRanges, TaskId, TaskReport, TaskStatus, TaskTimestamp},
};
use pwf_wire::{
    set_field::SetField,
    task::{ClosedTaskAction, TaskMutationSummary},
};

use crate::{
    ports::task_vault::{NullablePatch, TaskMutationError, TaskPatch, TaskVault, TaskWrite},
    task::{
        commit_task_writes, expected_task_revision, note_body::append_report, task_body_region,
    },
};

#[derive(Debug, thiserror::Error)]
pub enum CloseTaskError {
    #[error("Active task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error("Unknown project ID `{project_id}` for task {task_id}")]
    UnknownProjectId {
        task_id: TaskId,
        project_id: ProjectId,
    },
    #[error(transparent)]
    Revision(#[from] super::TaskRevisionConflict),
    #[error(transparent)]
    WriteStore(anyhow::Error),
    #[error(transparent)]
    Mutation(#[from] TaskMutationError<anyhow::Error>),
}

pub(in crate::task) struct TaskClosure<'a> {
    pub(in crate::task) action: ClosedTaskAction,
    pub(in crate::task) id: &'a TaskId,
    pub(in crate::task) completed_at: TaskTimestamp,
    pub(in crate::task) report: Option<&'a TaskReport>,
    pub(in crate::task) commits: Option<&'a CommitRanges>,
    pub(in crate::task) expected_revision: Option<&'a pwf_models::revision::ContentRevision>,
}

pub(in crate::task) fn close(
    command: &TaskClosure<'_>,
    store: &impl TaskVault,
    project: &pwf_models::project::Project,
) -> Result<TaskMutationSummary, CloseTaskError> {
    let TaskClosure {
        action,
        id,
        completed_at,
        report,
        commits,
        expected_revision,
    } = *command;
    let task_identifier = id.clone();
    if &project.id != task_identifier.project_id() {
        return Err(CloseTaskError::UnknownProjectId {
            project_id: task_identifier.project_id().clone(),
            task_id: task_identifier,
        });
    }
    let record = TaskVault::get_task_record(store, project, &task_identifier)
        .map_err(|error| CloseTaskError::WriteStore(anyhow::Error::new(error)))?
        .ok_or_else(|| CloseTaskError::TaskNotFound {
            id: task_identifier.clone(),
        })?;
    super::ensure_task_revision(expected_revision, &record)?;
    if record.status != TaskStatus::Active {
        return Err(CloseTaskError::TaskNotFound {
            id: task_identifier,
        });
    }
    let mut patch = TaskPatch {
        status: SetField::Set(close_status(action)),
        completed_at: NullablePatch::Set(completed_at),
        ..TaskPatch::default()
    };
    if let Some(report) = report {
        let body = append_report(task_body_region(&record.body), report.as_ref());
        patch.body = SetField::Set(body);
    }
    if let Some(commits) = commits {
        patch.commits = NullablePatch::Set(commits.to_string());
    }
    commit_task_writes(
        store,
        project,
        vec![expected_task_revision(&record)],
        vec![TaskWrite::Patch {
            id: task_identifier.clone(),
            patch,
        }],
    )?;

    Ok(TaskMutationSummary {
        id: task_identifier,
        title: record.title,
        status: close_status(action),
    })
}

fn close_status(action: ClosedTaskAction) -> TaskStatus {
    match action {
        ClosedTaskAction::Done => TaskStatus::Done,
        ClosedTaskAction::Cancelled => TaskStatus::Cancelled,
    }
}
