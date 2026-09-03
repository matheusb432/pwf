use pwf_models::{
    project::Project,
    task::{TaskId, TaskTitle, TaskTitleError},
};
use pwf_wire::{
    confirmation::RemoveTaskConfirmation,
    project::ProjectStatusFilter,
    task::{DeleteTask, DeleteTaskOutcome, TaskNotePath},
};

use super::{
    blocked_by, commit_task_writes,
    mutation_request::{self, MutationOperation, MutationRequestState, MutationStart},
    resolve_task_project::{self, ResolveTaskProjectError},
};
use crate::{
    ports::{
        confirmation::{ConfirmationClient, ConfirmationClientError},
        task_vault::{
            ExpectedTaskRevision, Materialization, StoredBlockedBy, TaskMutationError, TaskRecord,
            TaskVault, TaskWrite,
        },
    },
    project::list_projects,
};

#[derive(Debug, thiserror::Error)]
pub enum RemoveTaskError {
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

/// Deletes a task after unlinking its index entry.
///
/// An unlink failure leaves the note untouched.
#[cqrsy::command]
pub async fn execute(
    command: &DeleteTask,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
    confirmation_client: &mut dyn ConfirmationClient<Confirmation = RemoveTaskConfirmation>,
) -> Result<DeleteTaskOutcome, RemoveTaskError> {
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
        return Ok(DeleteTaskOutcome::Aborted);
    }
    if let Err(error) = validate_target_revision(&prepared, store) {
        if let Some(identity) = identity.as_ref() {
            mutation_request::discard(pool, identity, MutationOperation::Delete).await?;
        }
        return Err(error);
    }
    delete_prepared(&prepared, store)?;
    if let Some(identity) = identity.as_ref() {
        mutation_request::complete(pool, identity, MutationOperation::Delete, Some("deleted"))
            .await?;
    }
    Ok(DeleteTaskOutcome::Deleted)
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
    let record = TaskVault::get_task(store, &project, task_id)
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
    let confirmation = RemoveTaskConfirmation {
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
    ensure_no_dependents(&prepared.task_id, store, pool).await?;
    validate_target_revision(prepared, store)
}

fn validate_target_revision(
    prepared: &PreparedRemoval,
    store: &impl TaskVault,
) -> Result<(), RemoveTaskError> {
    let current = TaskVault::get_task(store, &prepared.project, &prepared.task_id)
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
            TaskWrite::DeleteIndex(prepared.task_id.clone()),
            TaskWrite::MoveToTrash {
                id: prepared.task_id.clone(),
            },
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
) -> Result<DeleteTaskOutcome, RemoveTaskError> {
    if replay.state == MutationRequestState::Pending {
        return Err(identity.incomplete().into());
    }
    match replay.outcome.as_deref() {
        Some("deleted") => Ok(DeleteTaskOutcome::Deleted),
        Some("aborted") => Ok(DeleteTaskOutcome::Aborted),
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

#[cfg(test)]
mod tests {
    use pwf_models::task::{TaskId, TaskStatus};
    use pwf_wire::{
        confirmation::RemoveTaskConfirmation,
        task::{DeleteTask, DeleteTaskOutcome, TaskNotePath},
    };

    use super::RemoveTaskError;
    use crate::{
        ports::{
            confirmation::{ConfirmationClient, ConfirmationClientError},
            task_vault::{IndexEntry, IndexEntryState, Materialization, TaskRecord, TaskVault},
        },
        task::remove_task,
        testing::{
            InMemoryStore, insert_project, project, stored_blocked_by, task_record, task_timestamp,
        },
    };

    async fn run(
        task_id: &TaskId,
        store: &InMemoryStore,
        pool: &sqlx::SqlitePool,
        confirmation: &mut dyn ConfirmationClient<Confirmation = RemoveTaskConfirmation>,
    ) -> Result<DeleteTaskOutcome, RemoveTaskError> {
        remove_task::execute(
            &DeleteTask {
                id: task_id.clone(),
                request_id: None,
                request_fingerprint: None,
            },
            store,
            pool,
            confirmation,
        )
        .await
    }

    fn record(id: &str, status: TaskStatus) -> TaskRecord {
        TaskRecord {
            title: "stale task".to_string(),
            status,
            created_at: Some(task_timestamp("2026-07-01T12:34:56Z")),
            locator: TaskNotePath::new(format!("/notes/foo/{id}.md").into()),
            ..task_record(id)
        }
    }

    fn staged(status: TaskStatus) -> InMemoryStore {
        let index_state = match status {
            TaskStatus::Active => IndexEntryState::Open,
            TaskStatus::Done | TaskStatus::Cancelled => {
                IndexEntryState::Done(Some(task_timestamp("2026-07-02T12:34:56Z")))
            }
        };
        let store = InMemoryStore::default()
            .with_project_id("foo", "FOO")
            .with_project("foo", vec![record("FOO-0001", status)]);
        TaskVault::upsert_index_entry(
            &store,
            &project("FOO", "foo"),
            IndexEntry {
                id: TaskId::try_new("FOO-0001").unwrap(),
                state: index_state,
                section: None,
            },
        )
        .unwrap();
        store
    }

    fn task_id(id: &str) -> TaskId {
        id.parse().unwrap()
    }

    struct Accepted;

    impl ConfirmationClient for Accepted {
        type Confirmation = RemoveTaskConfirmation;

        fn confirm<'a>(
            &'a mut self,
            _confirmation: &'a RemoveTaskConfirmation,
        ) -> futures::future::BoxFuture<'a, Result<bool, ConfirmationClientError>> {
            Box::pin(futures::future::ready(Ok(true)))
        }
    }

    struct EditThenAccept {
        store: InMemoryStore,
        project: pwf_models::project::Project,
        id: TaskId,
    }

    impl ConfirmationClient for EditThenAccept {
        type Confirmation = RemoveTaskConfirmation;

        fn confirm<'a>(
            &'a mut self,
            _confirmation: &'a RemoveTaskConfirmation,
        ) -> futures::future::BoxFuture<'a, Result<bool, ConfirmationClientError>> {
            self.store.externally_edit_task(&self.project, &self.id);
            Box::pin(futures::future::ready(Ok(true)))
        }
    }

    struct StaticInteraction {
        accepted: bool,
    }

    impl ConfirmationClient for StaticInteraction {
        type Confirmation = RemoveTaskConfirmation;

        fn confirm<'a>(
            &'a mut self,
            _confirmation: &'a RemoveTaskConfirmation,
        ) -> futures::future::BoxFuture<'a, Result<bool, ConfirmationClientError>> {
            Box::pin(futures::future::ready(Ok(self.accepted)))
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_deletes_record_and_index_entry(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = staged(TaskStatus::Active);

        let outcome = run(&task_id("FOO-0001"), &store, &pool, &mut Accepted)
            .await
            .unwrap();
        assert_eq!(outcome, DeleteTaskOutcome::Deleted);
        assert!(store.tasks("foo").is_empty(), "record must be deleted");
        assert!(
            store.entries("foo").is_empty(),
            "index entry must be unlinked"
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_rejects_a_task_edited_after_preflight_without_unlinking(
        pool: sqlx::SqlitePool,
    ) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = staged(TaskStatus::Active);
        let id = task_id("FOO-0001");
        let mut confirmation = EditThenAccept {
            store: store.clone(),
            project: project("FOO", "foo"),
            id: id.clone(),
        };

        let error = run(&id, &store, &pool, &mut confirmation)
            .await
            .unwrap_err();

        assert!(matches!(error, RemoveTaskError::Revision(_)));
        assert_eq!(store.tasks("foo").len(), 1);
        assert!(store.tasks("foo")[0].source.ends_with("external edit\n"));
        assert_eq!(store.entries("foo").len(), 1);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_reports_all_dependents_including_paused_projects_without_mutating(
        pool: sqlx::SqlitePool,
    ) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        insert_project(
            &pool,
            "AUX",
            "paused-project",
            "/projects/paused",
            "/tasks/paused",
            true,
        )
        .await;
        let local_dependent = TaskRecord {
            blocked_by: stored_blocked_by(&["FOO-0001"]),
            ..record("FOO-0003", TaskStatus::Done)
        };
        let paused_dependent = TaskRecord {
            blocked_by: stored_blocked_by(&["FOO-0001"]),
            ..record("AUX-0002", TaskStatus::Active)
        };
        let store = staged(TaskStatus::Active)
            .with_project(
                "foo",
                vec![record("FOO-0001", TaskStatus::Active), local_dependent],
            )
            .with_project("paused-project", vec![paused_dependent]);
        let before = store.tasks("foo");

        let error = run(&task_id("FOO-0001"), &store, &pool, &mut Accepted)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            RemoveTaskError::HasDependents { ref target, ref dependents }
                if target.as_ref() == "FOO-0001"
                    && dependents.iter().map(AsRef::as_ref).collect::<Vec<_>>()
                        == ["AUX-0002", "FOO-0003"]
        ));
        assert_eq!(store.tasks("foo"), before);
        assert_eq!(store.entries("foo").len(), 1);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_rejects_an_invalid_persisted_title_before_mutation(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = InMemoryStore::default()
            .with_project_id("foo", "FOO")
            .with_project(
                "foo",
                vec![TaskRecord {
                    title: "x".repeat(201),
                    ..record("FOO-0001", TaskStatus::Active)
                }],
            );

        let error = run(&task_id("FOO-0001"), &store, &pool, &mut Accepted)
            .await
            .unwrap_err();

        assert!(matches!(error, RemoveTaskError::InvalidTitle { .. }));
        assert_eq!(store.tasks("foo").len(), 1);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_deletes_an_unindexed_task(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = InMemoryStore::default()
            .with_project_id("foo", "FOO")
            .with_project("foo", vec![record("FOO-0001", TaskStatus::Active)]);

        let outcome = run(&task_id("FOO-0001"), &store, &pool, &mut Accepted)
            .await
            .unwrap();

        assert_eq!(outcome, DeleteTaskOutcome::Deleted);
        assert!(store.tasks("foo").is_empty());
        assert!(store.entries("foo").is_empty());
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_deletes_closed_items(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        for status in [TaskStatus::Done, TaskStatus::Cancelled] {
            let store = staged(status);

            let outcome = run(&task_id("FOO-0001"), &store, &pool, &mut Accepted)
                .await
                .unwrap();

            assert_eq!(outcome, DeleteTaskOutcome::Deleted);
            assert!(store.tasks("foo").is_empty(), "{status} record retained");
            assert!(store.entries("foo").is_empty(), "{status} index retained");
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_missing_item_preserves_requested_id(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = staged(TaskStatus::Active);

        let error = run(&task_id("FOO-9999"), &store, &pool, &mut Accepted)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            RemoveTaskError::TaskNotFound { ref id } if id.as_ref() == "FOO-9999"
        ));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_reports_an_unknown_project_id(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = staged(TaskStatus::Active);

        let error = run(&task_id("XYZ-0001"), &store, &pool, &mut Accepted)
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown project ID `XYZ` for task XYZ-0001"
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_rejects_missing_note_wikilink_with_its_path(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let ghost = TaskRecord {
            materialization: Materialization::MissingNote {
                expected: TaskNotePath::new("/notes/foo/FOO-0001.md".into()),
            },
            ..record("FOO-0001", TaskStatus::Active)
        };
        let store = InMemoryStore::default()
            .with_project_id("foo", "FOO")
            .with_project("foo", vec![ghost]);

        let error = run(&task_id("FOO-0001"), &store, &pool, &mut Accepted)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            RemoveTaskError::NoteMissing { ref path }
                if path.as_path() == std::path::Path::new("/notes/foo/FOO-0001.md")
        ));
        assert_eq!(
            error.to_string(),
            "Task note missing: /notes/foo/FOO-0001.md"
        );
    }

    mod confirmed_removal {
        use pwf_wire::task::DeleteTaskOutcome;

        use super::*;

        #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
        async fn remove_deletes_record_and_index_entry_after_confirmation(pool: sqlx::SqlitePool) {
            insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
            let store = staged(TaskStatus::Active);

            let outcome = run(
                &task_id("FOO-0001"),
                &store,
                &pool,
                &mut StaticInteraction { accepted: true },
            )
            .await
            .unwrap();
            assert_eq!(outcome, DeleteTaskOutcome::Deleted);
            assert!(store.tasks("foo").is_empty());
            assert!(store.entries("foo").is_empty());
        }

        #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
        async fn remove_decline_returns_aborted_without_mutating_task(pool: sqlx::SqlitePool) {
            insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
            let store = staged(TaskStatus::Active);

            let outcome = run(
                &task_id("FOO-0001"),
                &store,
                &pool,
                &mut StaticInteraction { accepted: false },
            )
            .await
            .unwrap();

            assert_eq!(outcome, DeleteTaskOutcome::Aborted);
            assert_eq!(store.tasks("foo").len(), 1);
            assert_eq!(store.entries("foo").len(), 1);
        }
    }
}
