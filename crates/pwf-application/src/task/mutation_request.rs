//! Durable retry identity for task mutations.

use pwf_models::task::TaskId;
use pwf_wire::task::{TaskRequestFingerprint, TaskRequestId};

#[derive(Debug, Clone)]
pub(super) struct MutationIdentity {
    request_id: TaskRequestId,
    fingerprint: TaskRequestFingerprint,
}

impl MutationIdentity {
    pub(super) fn request_id(&self) -> &TaskRequestId {
        &self.request_id
    }

    pub(super) fn incomplete(&self) -> MutationRequestError {
        MutationRequestError::Incomplete {
            request_id: self.request_id.to_string(),
        }
    }
}

pub(super) fn identity(
    request_id: Option<&TaskRequestId>,
    fingerprint: Option<&TaskRequestFingerprint>,
) -> Result<Option<MutationIdentity>, MutationRequestError> {
    match (request_id, fingerprint) {
        (None, None) => Ok(None),
        (Some(request_id), Some(fingerprint)) => Ok(Some(MutationIdentity {
            request_id: request_id.clone(),
            fingerprint: fingerprint.clone(),
        })),
        (Some(request_id), None) => Err(MutationRequestError::InvalidIdentity {
            request_id: request_id.to_string(),
        }),
        (None, Some(_)) => Err(MutationRequestError::InvalidIdentity {
            request_id: "<missing>".to_string(),
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MutationOperation {
    Create,
    Update,
    Cancel,
    Complete,
    Delete,
    Reopen,
}

impl MutationOperation {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Cancel => "cancel",
            Self::Complete => "complete",
            Self::Delete => "delete",
            Self::Reopen => "reopen",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MutationRequestState {
    Pending,
    Completed,
}

#[derive(Debug, Clone)]
pub(super) struct MutationRequestRecord {
    pub(super) task_id: TaskId,
    pub(super) state: MutationRequestState,
    pub(super) outcome: Option<String>,
    pub(super) result_task_id: Option<TaskId>,
}

#[derive(Debug, Clone)]
pub(super) enum MutationStart {
    Fresh,
    Existing(MutationRequestRecord),
}

#[derive(Debug, thiserror::Error)]
pub enum MutationRequestError {
    #[error("request ID {request_id:?} is missing its validated request fingerprint")]
    InvalidIdentity { request_id: String },
    #[error("request ID {request_id:?} is already bound to different mutation input")]
    Conflict { request_id: String },
    #[error(
        "request ID {request_id:?} has an incomplete prior attempt; inspect the task with GetTask before retrying with a new request ID"
    )]
    Incomplete { request_id: String },
    #[error("task mutation request {request_id:?} has invalid persisted state: {reason}")]
    Corrupt {
        request_id: String,
        reason: &'static str,
    },
    #[error(transparent)]
    Database(anyhow::Error),
}

pub(super) async fn find(
    pool: &sqlx::SqlitePool,
    identity: &MutationIdentity,
    operation: MutationOperation,
) -> Result<Option<MutationRequestRecord>, MutationRequestError> {
    let row = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
        ),
    >(
        "SELECT operation, fingerprint, task_id, state, outcome, result_task_id
         FROM task_mutation_requests
         WHERE request_id = ?",
    )
    .bind(identity.request_id.as_ref())
    .fetch_optional(pool)
    .await
    .map_err(|error| MutationRequestError::Database(anyhow::Error::new(error)))?;
    row.map(|row| decode_record(identity, operation, row))
        .transpose()
}

pub(super) async fn start(
    pool: &sqlx::SqlitePool,
    identity: &MutationIdentity,
    operation: MutationOperation,
    task_id: &TaskId,
) -> Result<MutationStart, MutationRequestError> {
    let inserted = sqlx::query(
        "INSERT INTO task_mutation_requests (request_id, operation, fingerprint, task_id)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(request_id) DO NOTHING",
    )
    .bind(identity.request_id.as_ref())
    .bind(operation.as_str())
    .bind(identity.fingerprint.as_ref())
    .bind(task_id.as_ref())
    .execute(pool)
    .await
    .map_err(|error| MutationRequestError::Database(anyhow::Error::new(error)))?;
    if inserted.rows_affected() == 1 {
        return Ok(MutationStart::Fresh);
    }
    let record =
        find(pool, identity, operation)
            .await?
            .ok_or_else(|| MutationRequestError::Corrupt {
                request_id: identity.request_id.to_string(),
                reason: "conflicting row disappeared",
            })?;
    Ok(MutationStart::Existing(record))
}

pub(super) async fn complete(
    pool: &sqlx::SqlitePool,
    identity: &MutationIdentity,
    operation: MutationOperation,
    outcome: Option<&str>,
    result_task_id: Option<&TaskId>,
) -> Result<(), MutationRequestError> {
    let updated = sqlx::query(
        "UPDATE task_mutation_requests
         SET state = 'completed', outcome = ?, result_task_id = ?,
             completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE request_id = ? AND operation = ? AND fingerprint = ? AND state = 'pending'",
    )
    .bind(outcome)
    .bind(result_task_id.map(AsRef::as_ref))
    .bind(identity.request_id.as_ref())
    .bind(operation.as_str())
    .bind(identity.fingerprint.as_ref())
    .execute(pool)
    .await
    .map_err(|error| MutationRequestError::Database(anyhow::Error::new(error)))?;
    if updated.rows_affected() == 0 {
        return Err(MutationRequestError::Corrupt {
            request_id: identity.request_id.to_string(),
            reason: "completion row is missing or changed",
        });
    }
    Ok(())
}

pub(super) async fn discard(
    pool: &sqlx::SqlitePool,
    identity: &MutationIdentity,
    operation: MutationOperation,
) -> Result<(), MutationRequestError> {
    sqlx::query(
        "DELETE FROM task_mutation_requests
         WHERE request_id = ? AND operation = ? AND fingerprint = ? AND state = 'pending'",
    )
    .bind(identity.request_id.as_ref())
    .bind(operation.as_str())
    .bind(identity.fingerprint.as_ref())
    .execute(pool)
    .await
    .map_err(|error| MutationRequestError::Database(anyhow::Error::new(error)))?;
    Ok(())
}

fn decode_record(
    identity: &MutationIdentity,
    operation: MutationOperation,
    row: (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
    ),
) -> Result<MutationRequestRecord, MutationRequestError> {
    let (stored_operation, stored_fingerprint, task_id, state, outcome, result_task_id) = row;
    if stored_operation != operation.as_str() || stored_fingerprint != identity.fingerprint.as_ref()
    {
        return Err(MutationRequestError::Conflict {
            request_id: identity.request_id.to_string(),
        });
    }
    let task_id = TaskId::try_new(task_id).map_err(|_| MutationRequestError::Corrupt {
        request_id: identity.request_id.to_string(),
        reason: "task_id is invalid",
    })?;
    let state = match state.as_str() {
        "pending" => MutationRequestState::Pending,
        "completed" => MutationRequestState::Completed,
        _ => {
            return Err(MutationRequestError::Corrupt {
                request_id: identity.request_id.to_string(),
                reason: "state is invalid",
            });
        }
    };
    let result_task_id = result_task_id
        .map(TaskId::try_new)
        .transpose()
        .map_err(|_| MutationRequestError::Corrupt {
            request_id: identity.request_id.to_string(),
            reason: "result_task_id is invalid",
        })?;
    Ok(MutationRequestRecord {
        task_id,
        state,
        outcome,
        result_task_id,
    })
}

#[cfg(test)]
mod tests {
    use pwf_wire::task::{TaskRequestFingerprint, TaskRequestId};

    use super::*;

    fn identity(byte: u8) -> MutationIdentity {
        MutationIdentity {
            request_id: TaskRequestId::try_new("request-1").unwrap(),
            fingerprint: TaskRequestFingerprint::from_digest([byte; 32]),
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn completed_request_replays_its_minimal_result(pool: sqlx::SqlitePool) {
        let identity = identity(1);
        let task_id = TaskId::try_new("FOO-0001").unwrap();
        let review_id = TaskId::try_new("FOO-0002").unwrap();

        assert!(matches!(
            start(&pool, &identity, MutationOperation::Complete, &task_id)
                .await
                .unwrap(),
            MutationStart::Fresh
        ));
        complete(
            &pool,
            &identity,
            MutationOperation::Complete,
            Some("completed"),
            Some(&review_id),
        )
        .await
        .unwrap();

        let replay = find(&pool, &identity, MutationOperation::Complete)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(replay.state, MutationRequestState::Completed);
        assert_eq!(replay.outcome.as_deref(), Some("completed"));
        assert_eq!(replay.result_task_id, Some(review_id));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn request_id_reuse_with_different_input_conflicts(pool: sqlx::SqlitePool) {
        let task_id = TaskId::try_new("FOO-0001").unwrap();
        start(&pool, &identity(1), MutationOperation::Update, &task_id)
            .await
            .unwrap();

        let error = find(&pool, &identity(2), MutationOperation::Update)
            .await
            .unwrap_err();
        assert!(matches!(error, MutationRequestError::Conflict { .. }));
    }
}
