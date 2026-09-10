use pwf_models::{
    project::Project,
    task::{TaskId, TaskTitle, TaskTitleError},
};
use pwf_wire::{
    confirmation::RemoveTaskConfirmation,
    project::ProjectStatusFilter,
    task::{
        DeleteTask, DeleteTaskOutcome, Materialization, StoredBlockedBy, TaskMutationResult,
        TaskMutationSummary, TaskNotePath, TaskRecord,
    },
};

use super::{
    blocked_by, commit_task_writes,
    mutation_request::{self, MutationOperation, MutationRequestState, MutationStart},
    resolve_task_project::{self, ResolveTaskProjectError},
};
use crate::{
    ports::{
        confirmation::{ConfirmationClient, ConfirmationClientError},
        task_vault::{ExpectedTaskRevision, TaskMutationError, TaskVault, TaskWrite},
    },
    project::list_projects,
};

#[derive(Debug, thiserror::Error)]
pub enum RemoveTaskError {
    #[error("project deletion configuration changed after confirmation; retry removal")]
    DeletionChanged,
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error(transparent)]
    ResolveProject(#[from] ResolveTaskProjectError),
    #[error(transparent)]
    Confirmation(#[from] ConfirmationClientError),
    #[error("Task note missing: {path}")]
    NoteMissing { path: TaskNotePath },
    #[error(transparent)]
    Revision(#[from] super::TaskRevisionConflict),
    #[error(transparent)]
    MutationRequest(#[from] mutation_request::MutationRequestError),
    #[error("task {id} has an invalid persisted title: {source}")]
    InvalidTitle {
        id: TaskId,
        #[source]
        source: TaskTitleError,
    },
    #[error(
        "cannot remove task {target}; dependent task(s): {}",
        blocked_by::format_task_ids(dependents)
    )]
    HasDependents {
        target: TaskId,
        dependents: Vec<TaskId>,
    },
    #[error("task {task} at {path} has malformed blocked_by metadata {raw:?}: {reason}")]
    MalformedBlockedBy {
        task: TaskId,
        path: Box<TaskNotePath>,
        raw: Box<str>,
        reason: Box<str>,
    },
    #[error("cannot inspect task dependents: {0}")]
    ReadDependents(#[source] anyhow::Error),
    #[error(transparent)]
    WriteStore(anyhow::Error),
    #[error(transparent)]
    Mutation(#[from] TaskMutationError<anyhow::Error>),
}

/// Confirms the deletion destination before removing the note and index entry.
#[cqrsy::command]
pub async fn execute(
    command: &DeleteTask,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
    confirmation_client: &mut dyn ConfirmationClient<Confirmation = RemoveTaskConfirmation>,
) -> Result<TaskMutationResult<DeleteTaskOutcome>, RemoveTaskError> {
    let identity = mutation_request::identity(
        command.request_id.as_ref(),
        command.request_fingerprint.as_ref(),
    )?;
    if let Some(identity) = identity.as_ref()
        && let Some(replay) =
            mutation_request::find(pool, identity, MutationOperation::Delete).await?
    {
        return delete_replay(&replay, identity);
    }

    let prepared = prepare_removal(&command.id, store, pool).await?;
    let confirmed = confirmation_client.confirm(&prepared.confirmation).await?;
    if confirmed {
        validate_removal(&prepared, store, pool).await?;
    }
    if let Some(identity) = identity.as_ref()
        && let MutationStart::Existing(replay) =
            mutation_request::start(pool, identity, MutationOperation::Delete, &command.id).await?
    {
        return delete_replay(&replay, identity);
    }
    if !confirmed {
        if let Some(identity) = identity.as_ref() {
            mutation_request::complete(pool, identity, MutationOperation::Delete, Some("aborted"))
                .await?;
        }
        return Ok(TaskMutationResult {
            outcome: DeleteTaskOutcome::Aborted,
            task: None,
        });
    }
    if let Err(error) = validate_target_revision(&prepared, store) {
        if let Some(identity) = identity.as_ref() {
            mutation_request::discard(pool, identity, MutationOperation::Delete).await?;
        }
        return Err(error);
    }
    let summary = TaskMutationSummary {
        id: prepared.task_id.clone(),
        title: prepared.confirmation.title.to_string(),
        status: prepared.confirmation.status,
    };
    delete_prepared(&prepared, store)?;
    if let Some(identity) = identity.as_ref() {
        mutation_request::complete_with_task(
            pool,
            identity,
            MutationOperation::Delete,
            "deleted",
            &summary,
        )
        .await?;
    }
    Ok(TaskMutationResult {
        outcome: DeleteTaskOutcome::Deleted,
        task: Some(summary),
    })
}

struct PreparedRemoval {
    project: Project,
    task_id: TaskId,
    confirmation: RemoveTaskConfirmation,
}

async fn prepare_removal(
    task_id: &TaskId,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
) -> Result<PreparedRemoval, RemoveTaskError> {
    let project = resolve_task_project::execute(task_id.clone(), pool).await?;
    let record = TaskVault::get_task_record(store, &project, task_id)
        .map_err(|error| RemoveTaskError::WriteStore(anyhow::Error::new(error)))?
        .ok_or_else(|| RemoveTaskError::TaskNotFound {
            id: task_id.clone(),
        })?;
    let note_path = match &record.materialization {
        Materialization::NoteFile => record.locator.clone(),
        Materialization::MissingNote { expected } => {
            return Err(RemoveTaskError::NoteMissing {
                path: expected.clone(),
            });
        }
    };
    let title = TaskTitle::try_new(record.title.clone()).map_err(|source| {
        RemoveTaskError::InvalidTitle {
            id: task_id.clone(),
            source,
        }
    })?;
    ensure_no_dependents(task_id, store, pool).await?;
    let deletion = store
        .task_deletion(&project)
        .map_err(|error| RemoveTaskError::WriteStore(anyhow::Error::new(error)))?;
    let confirmation = RemoveTaskConfirmation {
        deletion,
        task_identifier: task_id.clone(),
        project: project.title.clone(),
        title: title.clone(),
        status: record.status,
        note_path: note_path.clone(),
        revision: super::task_revision(&record),
    };
    Ok(PreparedRemoval {
        project,
        task_id: task_id.clone(),
        confirmation,
    })
}

async fn validate_removal(
    prepared: &PreparedRemoval,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
) -> Result<(), RemoveTaskError> {
    let project = resolve_task_project::execute(prepared.task_id.clone(), pool).await?;
    if project.obsidian_vault != prepared.project.obsidian_vault {
        return Err(RemoveTaskError::DeletionChanged);
    }
    store
        .task_deletion(&project)
        .map_err(|error| RemoveTaskError::WriteStore(anyhow::Error::new(error)))?;
    ensure_no_dependents(&prepared.task_id, store, pool).await?;
    validate_target_revision(prepared, store)
}

fn validate_target_revision(
    prepared: &PreparedRemoval,
    store: &impl TaskVault,
) -> Result<(), RemoveTaskError> {
    let current = TaskVault::get_task_record(store, &prepared.project, &prepared.task_id)
        .map_err(|error| RemoveTaskError::WriteStore(anyhow::Error::new(error)))?
        .ok_or_else(|| RemoveTaskError::TaskNotFound {
            id: prepared.task_id.clone(),
        })?;
    super::ensure_task_revision(Some(&prepared.confirmation.revision), &current)?;
    Ok(())
}

fn delete_prepared(
    prepared: &PreparedRemoval,
    store: &impl TaskVault,
) -> Result<(), RemoveTaskError> {
    commit_task_writes(
        store,
        &prepared.project,
        vec![ExpectedTaskRevision {
            id: prepared.task_id.clone(),
            revision: prepared.confirmation.revision.clone(),
        }],
        vec![
            TaskWrite::DeleteNote {
                deletion: prepared.confirmation.deletion.clone(),
                id: prepared.task_id.clone(),
            },
            TaskWrite::DeleteIndex(prepared.task_id.clone()),
        ],
    )
    .map_err(Into::into)
}

async fn ensure_no_dependents(
    task_id: &TaskId,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
) -> Result<(), RemoveTaskError> {
    let dependents = find_dependents(task_id, store, pool).await?;
    if !dependents.is_empty() {
        return Err(RemoveTaskError::HasDependents {
            target: task_id.clone(),
            dependents,
        });
    }
    Ok(())
}

fn delete_replay(
    replay: &mutation_request::MutationRequestRecord,
    identity: &mutation_request::MutationIdentity,
) -> Result<TaskMutationResult<DeleteTaskOutcome>, RemoveTaskError> {
    if replay.state == MutationRequestState::Pending {
        return Err(identity.incomplete().into());
    }
    match replay.outcome.as_deref() {
        Some("deleted") => Ok(TaskMutationResult {
            outcome: DeleteTaskOutcome::Deleted,
            task: replay.task.clone(),
        }),
        Some("aborted") => Ok(TaskMutationResult {
            outcome: DeleteTaskOutcome::Aborted,
            task: None,
        }),
        Some(_) | None => Err(mutation_request::MutationRequestError::Corrupt {
            request_id: identity.request_id().to_string(),
            reason: "delete outcome is invalid",
        }
        .into()),
    }
}

async fn find_dependents(
    target: &TaskId,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
) -> Result<Vec<TaskId>, RemoveTaskError> {
    let projects = list_projects::execute(ProjectStatusFilter::IncludingPaused, pool)
        .await
        .map_err(|error| RemoveTaskError::ReadDependents(anyhow::Error::new(error)))?;
    let mut dependents = Vec::new();
    for project in projects {
        dependents.extend(project_dependents(target, store, &project)?);
    }
    dependents.sort();
    dependents.dedup();
    Ok(dependents)
}

fn project_dependents(
    target: &TaskId,
    store: &impl TaskVault,
    project: &Project,
) -> Result<Vec<TaskId>, RemoveTaskError> {
    let records = store
        .list_tasks(project)
        .map_err(|error| RemoveTaskError::ReadDependents(anyhow::Error::new(error)))?;
    let candidates = records
        .into_iter()
        .map(|record| dependent_id(record, target))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(candidates.into_iter().flatten().collect())
}

fn dependent_id(record: TaskRecord, target: &TaskId) -> Result<Option<TaskId>, RemoveTaskError> {
    match record.blocked_by {
        StoredBlockedBy::Valid(blocked_by) => Ok(blocked_by
            .iter()
            .any(|blocker| blocker == target)
            .then_some(record.id)),
        StoredBlockedBy::Absent => Ok(None),
        StoredBlockedBy::Malformed { raw, reason } => Err(RemoveTaskError::MalformedBlockedBy {
            task: record.id,
            path: Box::new(record.locator),
            raw: raw.into_boxed_str(),
            reason: reason.into_boxed_str(),
        }),
    }
}
