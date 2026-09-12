use pwf_models::task::{TaskStatus, TaskTimestamp};
use pwf_wire::{
    confirmation::ActivateTaskConfirmation,
    set_field::SetField,
    task::{ActivateTask, ActivateTaskOutcome, TaskMutationResult, TaskMutationSummary},
};

use super::{
    commit_task_writes,
    content::remove_report,
    expected_task_revision,
    get_task_record::{self, GetTaskRecordError, LoadedTaskRecord},
    task_body_region,
};
use crate::ports::{
    confirmation::{ConfirmationClient, ConfirmationClientError},
    task_vault::{NullablePatch, TaskMutationError, TaskPatch, TaskVault, TaskWrite},
};

#[derive(Debug, thiserror::Error)]
pub enum ActivateTaskError {
    #[error(transparent)]
    Read(#[from] GetTaskRecordError),
    #[error(transparent)]
    Confirmation(#[from] ConfirmationClientError),
    #[error(transparent)]
    Mutation(#[from] TaskMutationError<anyhow::Error>),
}

/// Activates a task, confirming removal of completion data only for closed tasks.
#[cqrsy::command]
pub async fn execute(
    command: &ActivateTask,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
    confirmation_client: &mut dyn ConfirmationClient<Confirmation = ActivateTaskConfirmation>,
) -> Result<TaskMutationResult<ActivateTaskOutcome>, ActivateTaskError> {
    let LoadedTaskRecord { project, record } =
        get_task_record::load(&command.id, store, pool).await?;
    let summary = TaskMutationSummary {
        id: command.id.clone(),
        title: record.title.clone(),
        status: TaskStatus::Active,
    };
    let mut patch = TaskPatch {
        status: SetField::Set(TaskStatus::Active),
        ..TaskPatch::default()
    };
    match ActivationAction::from(record.status) {
        ActivationAction::AlreadyActive => {
            return Ok(TaskMutationResult {
                outcome: ActivateTaskOutcome::AlreadyActive,
                task: Some(summary),
            });
        }
        ActivationAction::Activate => {}
        ActivationAction::ConfirmCompletionRemoval => {
            let (body_without_report, report) = remove_report(task_body_region(&record.body));
            let confirmation = ActivateTaskConfirmation {
                task_identifier: command.id.clone(),
                project: project.title.clone(),
                completion_date: record.completed_at.map(TaskTimestamp::date),
                commit_provenance: record.commits.clone(),
                report: report.clone(),
            };
            if !confirmation_client.confirm(&confirmation).await? {
                return Ok(TaskMutationResult {
                    outcome: ActivateTaskOutcome::Aborted,
                    task: None,
                });
            }
            patch.completed_at = NullablePatch::Clear;
            patch.commits = NullablePatch::Clear;
            patch.body = report.is_some().then_some(body_without_report).into();
        }
    }
    commit_task_writes(
        store,
        &project,
        vec![expected_task_revision(&record)],
        vec![TaskWrite::Patch {
            id: command.id.clone(),
            patch,
        }],
    )?;
    Ok(TaskMutationResult {
        outcome: ActivateTaskOutcome::Activated,
        task: Some(summary),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivationAction {
    AlreadyActive,
    Activate,
    ConfirmCompletionRemoval,
}

impl From<TaskStatus> for ActivationAction {
    fn from(status: TaskStatus) -> Self {
        match status {
            TaskStatus::Active => Self::AlreadyActive,
            TaskStatus::Backlog => Self::Activate,
            TaskStatus::Done | TaskStatus::Cancelled => Self::ConfirmCompletionRemoval,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ActivationAction, TaskStatus};

    #[test]
    fn activation_requires_confirmation_only_for_closed_tasks() {
        for (status, action) in [
            (TaskStatus::Active, ActivationAction::AlreadyActive),
            (TaskStatus::Backlog, ActivationAction::Activate),
            (TaskStatus::Done, ActivationAction::ConfirmCompletionRemoval),
            (
                TaskStatus::Cancelled,
                ActivationAction::ConfirmCompletionRemoval,
            ),
        ] {
            assert_eq!(ActivationAction::from(status), action, "{status}");
        }
    }
}
