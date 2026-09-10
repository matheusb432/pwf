use pwf_models::task::{TaskId, TaskStatus, TaskTimestamp};
use pwf_wire::{
    confirmation::ReopenTaskConfirmation,
    set_field::SetField,
    task::{ReopenTask, ReopenTaskOutcome, TaskMutationResult, TaskMutationSummary},
};

use super::{
    commit_task_writes,
    note_body::remove_report,
    resolve_task_project::{self, ResolveTaskProjectError},
    task_body_region,
};
use crate::ports::{
    confirmation::{ConfirmationClient, ConfirmationClientError},
    task_vault::{
        ExpectedTaskRevision, NullablePatch, TaskMutationError, TaskPatch, TaskVault, TaskWrite,
    },
};

#[derive(Debug, thiserror::Error)]
pub enum ReopenTaskError {
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error(transparent)]
    ResolveProject(#[from] ResolveTaskProjectError),
    #[error(transparent)]
    Confirmation(#[from] ConfirmationClientError),
    #[error(transparent)]
    Revision(#[from] super::TaskRevisionConflict),
    #[error(transparent)]
    WriteStore(anyhow::Error),
    #[error(transparent)]
    Mutation(#[from] TaskMutationError<anyhow::Error>),
}

/// Reopens a closed task note.
///
/// The update clears completion metadata and its appended report. An active task returns an
/// idempotent skip.
#[cqrsy::command]
pub async fn execute(
    command: &ReopenTask,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
    confirmation_client: &mut dyn ConfirmationClient<Confirmation = ReopenTaskConfirmation>,
) -> Result<TaskMutationResult<ReopenTaskOutcome>, ReopenTaskError> {
    let prepared = match prepare_reopen(&command.id, store, pool).await? {
        ReopenPreparation::Closed(prepared) => prepared,
        ReopenPreparation::AlreadyActive(summary) => {
            return Ok(TaskMutationResult {
                outcome: ReopenTaskOutcome::AlreadyActive,
                task: Some(summary),
            });
        }
    };
    if !confirmation_client.confirm(&prepared.confirmation).await? {
        return Ok(TaskMutationResult {
            outcome: ReopenTaskOutcome::Aborted,
            task: None,
        });
    }
    validate_reopen(&prepared, store)?;
    let summary = prepared.summary.clone();
    apply_reopen(*prepared, store)?;
    Ok(TaskMutationResult {
        outcome: ReopenTaskOutcome::Reopened,
        task: Some(summary),
    })
}

enum ReopenPreparation {
    AlreadyActive(TaskMutationSummary),
    Closed(Box<PreparedReopen>),
}

struct PreparedReopen {
    summary: TaskMutationSummary,
    project: pwf_models::project::Project,
    task_id: TaskId,
    body_without_report: String,
    report: Option<String>,
    confirmation: ReopenTaskConfirmation,
}

async fn prepare_reopen(
    task_id: &TaskId,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
) -> Result<ReopenPreparation, ReopenTaskError> {
    let project = resolve_task_project::execute(task_id.clone(), pool).await?;
    let record = TaskVault::get_task_record(store, &project, task_id)
        .map_err(|error| ReopenTaskError::WriteStore(anyhow::Error::new(error)))?
        .ok_or_else(|| ReopenTaskError::TaskNotFound {
            id: task_id.clone(),
        })?;
    let summary = TaskMutationSummary {
        id: task_id.clone(),
        title: record.title.clone(),
        status: TaskStatus::Active,
    };
    if record.status == TaskStatus::Active {
        return Ok(ReopenPreparation::AlreadyActive(summary));
    }
    let (body_without_report, report) = remove_report(task_body_region(&record.body));
    let confirmation = ReopenTaskConfirmation {
        task_identifier: task_id.clone(),
        project: project.title.clone(),
        completion_date: record.completed_at.map(TaskTimestamp::date),
        commit_provenance: record.commits.clone(),
        report: report.clone(),
        revision: super::task_revision(&record),
    };
    Ok(ReopenPreparation::Closed(Box::new(PreparedReopen {
        summary,
        project,
        task_id: task_id.clone(),
        body_without_report,
        report,
        confirmation,
    })))
}

fn validate_reopen(
    prepared: &PreparedReopen,
    store: &impl TaskVault,
) -> Result<(), ReopenTaskError> {
    let current = TaskVault::get_task_record(store, &prepared.project, &prepared.task_id)
        .map_err(|error| ReopenTaskError::WriteStore(anyhow::Error::new(error)))?
        .ok_or_else(|| ReopenTaskError::TaskNotFound {
            id: prepared.task_id.clone(),
        })?;
    super::ensure_task_revision(Some(&prepared.confirmation.revision), &current)?;
    Ok(())
}

fn apply_reopen(prepared: PreparedReopen, store: &impl TaskVault) -> Result<(), ReopenTaskError> {
    let task_identifier = prepared.task_id;
    let patch = TaskWrite::Patch {
        id: task_identifier.clone(),
        patch: TaskPatch {
            status: SetField::Set(TaskStatus::Active),
            completed_at: NullablePatch::Clear,
            commits: NullablePatch::Clear,
            body: prepared
                .report
                .is_some()
                .then_some(prepared.body_without_report)
                .into(),
            ..TaskPatch::default()
        },
    };

    commit_task_writes(
        store,
        &prepared.project,
        vec![ExpectedTaskRevision {
            id: task_identifier,
            revision: prepared.confirmation.revision,
        }],
        vec![patch],
    )
    .map_err(Into::into)
}
