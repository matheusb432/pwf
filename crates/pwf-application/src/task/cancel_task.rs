use pwf_models::task::{TaskId, TaskTimestampError};
use pwf_wire::task::{CancelTask, ClosedTaskAction};

use super::{
    CloseTaskError, TaskPromptLanesError,
    lane_configuration::TaskPromptLanes,
    mutation_request::{self, MutationOperation, MutationRequestState, MutationStart},
    resolve_task_project::{self, ResolveTaskProjectError},
    task_closure::{self, TaskClosure},
};
use crate::ports::{
    clock::Clock,
    task_record::{
        IndexEntryStore, IndexSectionStore, TaskMutationError, TaskMutationStore, TaskStore,
    },
};

#[derive(Debug, thiserror::Error)]
pub enum CancelTaskError {
    #[error(transparent)]
    ResolveProject(#[from] ResolveTaskProjectError),
    #[error(transparent)]
    Close(#[from] CloseTaskError),
    #[error("cannot read the task cancellation time: {0}")]
    Clock(#[from] TaskTimestampError),
    #[error(transparent)]
    MutationRequest(#[from] mutation_request::MutationRequestError),
    #[error(transparent)]
    PromptLanes(#[from] TaskPromptLanesError),
}

#[cqrsy::command]
pub async fn execute(
    command: &CancelTask,
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore + TaskMutationStore),
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<Option<TaskId>, CancelTaskError> {
    let identity = mutation_request::identity(
        command.request_id.as_ref(),
        command.request_fingerprint.as_ref(),
    )?;
    if let Some(identity) = identity.as_ref()
        && let Some(replay) =
            mutation_request::find(pool, identity, MutationOperation::Cancel).await?
    {
        return match replay.state {
            MutationRequestState::Completed => Ok(replay.result_task_id),
            MutationRequestState::Pending => Err(identity.incomplete().into()),
        };
    }
    let project = resolve_task_project::execute(command.id.clone(), pool).await?;
    let review_lanes = if command.review {
        Some(TaskPromptLanes::load(pool).await?)
    } else {
        None
    };
    let completed_at = clock.now()?;
    if let Some(identity) = identity.as_ref()
        && let MutationStart::Existing(replay) =
            mutation_request::start(pool, identity, MutationOperation::Cancel, &command.id).await?
    {
        return match replay.state {
            MutationRequestState::Completed => Ok(replay.result_task_id),
            MutationRequestState::Pending => Err(identity.incomplete().into()),
        };
    }
    let result = task_closure::close(
        &TaskClosure {
            action: ClosedTaskAction::Cancelled,
            id: &command.id,
            completed_at,
            report: Some(&command.report),
            commits: command.commits.as_ref(),
            review_lanes: review_lanes.as_ref(),
            expected_revision: command.expected_revision.as_ref(),
        },
        store,
        &project,
    );
    let effects = match result {
        Ok(effects) => effects,
        Err(error) => {
            if let Some(identity) = identity.as_ref()
                && close_failed_before_mutation(&error)
            {
                mutation_request::discard(pool, identity, MutationOperation::Cancel).await?;
            }
            return Err(error.into());
        }
    };
    if let Some(identity) = identity.as_ref() {
        mutation_request::complete(
            pool,
            identity,
            MutationOperation::Cancel,
            Some("cancelled"),
            effects.review_task.as_ref().map(|task| &task.id),
        )
        .await?;
    }
    Ok(effects.review_task.map(|task| task.id))
}

fn close_failed_before_mutation(error: &CloseTaskError) -> bool {
    matches!(
        error,
        CloseTaskError::TaskNotFound { .. }
            | CloseTaskError::UnknownProjectId { .. }
            | CloseTaskError::Revision(_)
            | CloseTaskError::Mutation(
                TaskMutationError::StaleTask { .. } | TaskMutationError::SourceChanged
            )
    )
}

#[cfg(test)]
mod tests {
    use pwf_models::task::{TaskId, TaskStatus};

    use super::CancelTask;
    use crate::{
        ports::task_record::{IndexEntry, IndexEntryState, IndexEntryStore, TaskRecord},
        task::cancel_task,
        testing::{FixedClock, InMemoryStore, project, task_record, task_timestamp},
    };

    fn record(id: &str) -> TaskRecord {
        task_record(id)
    }

    fn staged() -> InMemoryStore {
        let store = InMemoryStore::default()
            .with_project_id("foo-bar", "FOO")
            .with_project("foo-bar", vec![record("FOO-0001")]);
        IndexEntryStore::upsert_index_entry(
            &store,
            &project("FOO", "foo-bar"),
            IndexEntry {
                id: TaskId::try_new("FOO-0001").unwrap(),
                state: IndexEntryState::Open,
                section: None,
            },
        )
        .unwrap();
        store
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn cancel_marks_item_cancelled(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged();
        let command = CancelTask {
            id: "FOO-0001".parse().unwrap(),
            report: "obsoleted".parse().unwrap(),
            commits: "a..b, c..d".parse().ok(),
            review: false,
            expected_revision: None,
            request_id: None,
            request_fingerprint: None,
        };

        let out = cancel_task::execute(&command, &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(out, None);
        assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Cancelled);
        assert_eq!(
            store.tasks("foo-bar")[0].completed_at,
            Some(task_timestamp("2026-07-26T12:34:56Z"))
        );
        assert_eq!(
            store.tasks("foo-bar")[0].commits.as_deref(),
            Some("a..b, c..d")
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn cancel_uses_the_clock_timestamp(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged();
        let command = CancelTask {
            id: "FOO-0001".parse().unwrap(),
            report: "obsoleted".parse().unwrap(),
            commits: None,
            review: false,
            expected_revision: None,
            request_id: None,
            request_fingerprint: None,
        };

        cancel_task::execute(&command, &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(
            store.tasks("foo-bar")[0].completed_at,
            Some(task_timestamp("2026-07-26T12:34:56Z"))
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn cancel_reports_an_unknown_project_id(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let command = CancelTask {
            id: "XYZ-0001".parse().unwrap(),
            report: "obsolete".parse().unwrap(),
            commits: None,
            review: false,
            expected_revision: None,
            request_id: None,
            request_fingerprint: None,
        };

        let error = cancel_task::execute(&command, &staged(), &pool, &FixedClock)
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown project ID `XYZ` for task XYZ-0001"
        );
    }
}
