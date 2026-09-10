use pwf_models::task::{TaskId, TaskStatus, TaskTimestamp};
use pwf_wire::{
    confirmation::ReopenTaskConfirmation,
    set_field::SetField,
    task::{ReopenTask, ReopenTaskOutcome, TaskMutationResult, TaskMutationSummary},
};

use super::{
    commit_task_writes,
    mutation_request::{self, MutationOperation, MutationRequestState, MutationStart},
    note_body::remove_report,
    resolve_task_project::{self, ResolveTaskProjectError},
    task_body_region,
};
use crate::ports::{
    confirmation::{ConfirmationClient, ConfirmationClientError},
    task_vault::{
        ExpectedTaskRevision, IndexEntry, IndexEntryState, NullablePatch, TaskMutationError,
        TaskPatch, TaskVault, TaskWrite,
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
    MutationRequest(#[from] mutation_request::MutationRequestError),
    #[error(transparent)]
    WriteStore(anyhow::Error),
    #[error(transparent)]
    Mutation(#[from] TaskMutationError<anyhow::Error>),
}

/// Reopens a closed task and restores an existing queue link.
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
    let identity = mutation_request::identity(
        command.request_id.as_ref(),
        command.request_fingerprint.as_ref(),
    )?;
    if let Some(identity) = identity.as_ref()
        && let Some(replay) =
            mutation_request::find(pool, identity, MutationOperation::Reopen).await?
    {
        return reopen_replay(&replay, identity);
    }

    let prepared = match prepare_reopen(&command.id, store, pool).await? {
        ReopenPreparation::Closed(prepared) => prepared,
        ReopenPreparation::AlreadyActive(summary) => {
            if let Some(identity) = identity.as_ref()
                && let MutationStart::Existing(replay) =
                    mutation_request::start(pool, identity, MutationOperation::Reopen, &command.id)
                        .await?
            {
                return reopen_replay(&replay, identity);
            }
            if let Some(identity) = identity.as_ref() {
                mutation_request::complete_with_task(
                    pool,
                    identity,
                    MutationOperation::Reopen,
                    "already_active",
                    &summary,
                )
                .await?;
            }
            return Ok(TaskMutationResult {
                outcome: ReopenTaskOutcome::AlreadyActive,
                task: Some(summary),
            });
        }
    };
    let confirmed = confirmation_client.confirm(&prepared.confirmation).await?;
    if confirmed {
        validate_reopen(&prepared, store)?;
    }
    if let Some(identity) = identity.as_ref()
        && let MutationStart::Existing(replay) =
            mutation_request::start(pool, identity, MutationOperation::Reopen, &command.id).await?
    {
        return reopen_replay(&replay, identity);
    }
    if !confirmed {
        if let Some(identity) = identity.as_ref() {
            mutation_request::complete(pool, identity, MutationOperation::Reopen, Some("aborted"))
                .await?;
        }
        return Ok(TaskMutationResult {
            outcome: ReopenTaskOutcome::Aborted,
            task: None,
        });
    }
    if let Err(error) = validate_reopen(&prepared, store) {
        if let Some(identity) = identity.as_ref() {
            mutation_request::discard(pool, identity, MutationOperation::Reopen).await?;
        }
        return Err(error);
    }
    let summary = prepared.summary.clone();
    apply_reopen(*prepared, store)?;
    if let Some(identity) = identity.as_ref() {
        mutation_request::complete_with_task(
            pool,
            identity,
            MutationOperation::Reopen,
            "reopened",
            &summary,
        )
        .await?;
    }
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

    let entries = TaskVault::list_index_entries(store, &prepared.project)
        .map_err(|error| ReopenTaskError::WriteStore(anyhow::Error::new(error)))?;
    let mut writes = vec![patch];
    if entries
        .iter()
        .any(|entry| entry.id == task_identifier && matches!(entry.state, IndexEntryState::Done(_)))
    {
        writes.push(TaskWrite::UpsertIndex(IndexEntry {
            id: task_identifier.clone(),
            state: IndexEntryState::Open,
            section: None,
        }));
    }
    commit_task_writes(
        store,
        &prepared.project,
        vec![ExpectedTaskRevision {
            id: task_identifier,
            revision: prepared.confirmation.revision,
        }],
        writes,
    )
    .map_err(Into::into)
}

fn reopen_replay(
    replay: &mutation_request::MutationRequestRecord,
    identity: &mutation_request::MutationIdentity,
) -> Result<TaskMutationResult<ReopenTaskOutcome>, ReopenTaskError> {
    if replay.state == MutationRequestState::Pending {
        return Err(identity.incomplete().into());
    }
    match replay.outcome.as_deref() {
        Some("reopened") => Ok(TaskMutationResult {
            outcome: ReopenTaskOutcome::Reopened,
            task: replay.task.clone(),
        }),
        Some("already_active") => Ok(TaskMutationResult {
            outcome: ReopenTaskOutcome::AlreadyActive,
            task: replay.task.clone(),
        }),
        Some("aborted") => Ok(TaskMutationResult {
            outcome: ReopenTaskOutcome::Aborted,
            task: None,
        }),
        Some(_) | None => Err(mutation_request::MutationRequestError::Corrupt {
            request_id: identity.request_id().to_string(),
            reason: "reopen outcome is invalid",
        }
        .into()),
    }
}
