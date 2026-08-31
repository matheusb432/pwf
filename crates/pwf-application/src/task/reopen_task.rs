use pwf_models::task::{TaskId, TaskStatus, TaskTimestamp};
use pwf_wire::{
    confirmation::ReopenTaskConfirmation,
    task::{ReopenTask, ReopenTaskOutcome},
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
    task_record::{
        ExpectedTaskRevision, IndexEntry, IndexEntryState, IndexEntryStore, NullablePatch,
        TaskMutationError, TaskMutationStore, TaskPatch, TaskStore, TaskWrite,
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
    store: &(impl TaskStore + IndexEntryStore + TaskMutationStore),
    pool: &sqlx::SqlitePool,
    confirmation_client: &mut dyn ConfirmationClient<Confirmation = ReopenTaskConfirmation>,
) -> Result<ReopenTaskOutcome, ReopenTaskError> {
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

    let prepared = prepare_reopen(&command.id, store, pool).await?;
    let ReopenPreparation::Closed(prepared) = prepared else {
        if let Some(identity) = identity.as_ref()
            && let MutationStart::Existing(replay) =
                mutation_request::start(pool, identity, MutationOperation::Reopen, &command.id)
                    .await?
        {
            return reopen_replay(&replay, identity);
        }
        if let Some(identity) = identity.as_ref() {
            mutation_request::complete(
                pool,
                identity,
                MutationOperation::Reopen,
                Some("already_active"),
                None,
            )
            .await?;
        }
        return Ok(ReopenTaskOutcome::AlreadyActive);
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
            mutation_request::complete(
                pool,
                identity,
                MutationOperation::Reopen,
                Some("aborted"),
                None,
            )
            .await?;
        }
        return Ok(ReopenTaskOutcome::Aborted);
    }
    if let Err(error) = validate_reopen(&prepared, store) {
        if let Some(identity) = identity.as_ref() {
            mutation_request::discard(pool, identity, MutationOperation::Reopen).await?;
        }
        return Err(error);
    }
    apply_reopen(*prepared, store)?;
    if let Some(identity) = identity.as_ref() {
        mutation_request::complete(
            pool,
            identity,
            MutationOperation::Reopen,
            Some("reopened"),
            None,
        )
        .await?;
    }
    Ok(ReopenTaskOutcome::Reopened)
}

enum ReopenPreparation {
    AlreadyActive,
    Closed(Box<PreparedReopen>),
}

struct PreparedReopen {
    project: pwf_models::project::Project,
    task_id: TaskId,
    body_without_report: String,
    report: Option<String>,
    confirmation: ReopenTaskConfirmation,
}

async fn prepare_reopen(
    task_id: &TaskId,
    store: &impl TaskStore,
    pool: &sqlx::SqlitePool,
) -> Result<ReopenPreparation, ReopenTaskError> {
    let project = resolve_task_project::execute(task_id.clone(), pool).await?;
    let record = TaskStore::get(store, &project, task_id)
        .map_err(|error| ReopenTaskError::WriteStore(anyhow::Error::new(error)))?
        .ok_or_else(|| ReopenTaskError::TaskNotFound {
            id: task_id.clone(),
        })?;
    if record.status == TaskStatus::Active {
        return Ok(ReopenPreparation::AlreadyActive);
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
        project,
        task_id: task_id.clone(),
        body_without_report,
        report,
        confirmation,
    })))
}

fn validate_reopen(
    prepared: &PreparedReopen,
    store: &impl TaskStore,
) -> Result<(), ReopenTaskError> {
    let current = TaskStore::get(store, &prepared.project, &prepared.task_id)
        .map_err(|error| ReopenTaskError::WriteStore(anyhow::Error::new(error)))?
        .ok_or_else(|| ReopenTaskError::TaskNotFound {
            id: prepared.task_id.clone(),
        })?;
    super::ensure_task_revision(Some(&prepared.confirmation.revision), &current)?;
    Ok(())
}

fn apply_reopen(
    prepared: PreparedReopen,
    store: &(impl IndexEntryStore + TaskMutationStore),
) -> Result<(), ReopenTaskError> {
    let task_identifier = prepared.task_id;
    let patch = TaskWrite::Patch {
        id: task_identifier.clone(),
        patch: TaskPatch {
            status: Some(TaskStatus::Active),
            completed_at: NullablePatch::Clear,
            commits: NullablePatch::Clear,
            body: prepared
                .report
                .is_some()
                .then_some(prepared.body_without_report),
            ..TaskPatch::default()
        },
    };

    let entries = IndexEntryStore::list_index_entries(store, &prepared.project)
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
) -> Result<ReopenTaskOutcome, ReopenTaskError> {
    if replay.state == MutationRequestState::Pending {
        return Err(identity.incomplete().into());
    }
    match replay.outcome.as_deref() {
        Some("reopened") => Ok(ReopenTaskOutcome::Reopened),
        Some("already_active") => Ok(ReopenTaskOutcome::AlreadyActive),
        Some("aborted") => Ok(ReopenTaskOutcome::Aborted),
        Some(_) | None => Err(mutation_request::MutationRequestError::Corrupt {
            request_id: identity.request_id().to_string(),
            reason: "reopen outcome is invalid",
        }
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use pwf_models::{
        project::Project,
        task::{TaskId, TaskStatus},
    };
    use pwf_wire::{
        confirmation::ReopenTaskConfirmation,
        task::{ReopenTask, ReopenTaskOutcome},
    };

    use crate::{
        ports::{
            confirmation::{ConfirmationClient, ConfirmationClientError},
            task_record::{IndexEntry, IndexEntryState, IndexEntryStore, TaskRecord},
        },
        task::reopen_task,
        testing::{InMemoryStore, app_date, project, task_record, task_timestamp},
    };

    struct TestConfirmation {
        accepted: bool,
        recorded: Vec<ReopenTaskConfirmation>,
    }

    impl TestConfirmation {
        fn accepting() -> Self {
            Self {
                accepted: true,
                recorded: Vec::new(),
            }
        }

        fn declining() -> Self {
            Self {
                accepted: false,
                recorded: Vec::new(),
            }
        }

        fn recorded(&self) -> &[ReopenTaskConfirmation] {
            &self.recorded
        }
    }

    impl ConfirmationClient for TestConfirmation {
        type Confirmation = ReopenTaskConfirmation;

        fn confirm<'a>(
            &'a mut self,
            confirmation: &'a ReopenTaskConfirmation,
        ) -> futures::future::BoxFuture<'a, Result<bool, ConfirmationClientError>> {
            self.recorded.push(confirmation.clone());
            Box::pin(futures::future::ready(Ok(self.accepted)))
        }
    }

    struct EditThenAccept {
        store: InMemoryStore,
        project: Project,
        id: TaskId,
    }

    impl ConfirmationClient for EditThenAccept {
        type Confirmation = ReopenTaskConfirmation;

        fn confirm<'a>(
            &'a mut self,
            _confirmation: &'a ReopenTaskConfirmation,
        ) -> futures::future::BoxFuture<'a, Result<bool, ConfirmationClientError>> {
            self.store.externally_edit_task(&self.project, &self.id);
            Box::pin(futures::future::ready(Ok(true)))
        }
    }

    fn record(id: &str, status: TaskStatus) -> TaskRecord {
        TaskRecord {
            status,
            completed_at: (status != TaskStatus::Active)
                .then(|| task_timestamp("2026-01-02T12:34:56Z")),
            commits: Some("a..b".to_string()),
            body: if status == TaskStatus::Active {
                "## Goals\n\n- ship the work\n".to_string()
            } else {
                "## Goals\n\n- ship the work\n\n### Report\n\ncompleted safely\n".to_string()
            },
            ..task_record(id)
        }
    }

    fn foo() -> Project {
        project("FOO", "foo-bar")
    }

    fn staged(status: TaskStatus, entries: Vec<IndexEntry>) -> InMemoryStore {
        let store = InMemoryStore::default()
            .with_project_id("foo-bar", "FOO")
            .with_project("foo-bar", vec![record("FOO-0001", status)]);
        for entry in entries {
            IndexEntryStore::upsert_index_entry(&store, &foo(), entry).unwrap();
        }
        store
    }

    fn entry(state: IndexEntryState) -> IndexEntry {
        IndexEntry {
            id: TaskId::try_new("FOO-0001").unwrap(),
            state,
            section: None,
        }
    }

    fn command(id: &str) -> ReopenTask {
        ReopenTask {
            id: id.parse().unwrap(),
            request_id: None,
            request_fingerprint: None,
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn reopen_restores_done_entry(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(
            TaskStatus::Done,
            vec![entry(IndexEntryState::Done(Some(task_timestamp(
                "2026-01-02T12:34:56Z",
            ))))],
        );
        let mut confirmation = TestConfirmation::accepting();

        let out = reopen_task::execute(&command("FOO-0001"), &store, &pool, &mut confirmation)
            .await
            .unwrap();

        assert_eq!(out, ReopenTaskOutcome::Reopened);
        let confirmations = confirmation.recorded();
        assert_eq!(confirmations.len(), 1);
        let Some(confirmation) = confirmations.first() else {
            return;
        };
        assert_eq!(confirmation.task_identifier.as_ref(), "FOO-0001");
        assert_eq!(confirmation.project.as_ref(), "foo-bar");
        assert_eq!(confirmation.completion_date, Some(app_date("2026-01-02")));
        assert_eq!(confirmation.commit_provenance.as_deref(), Some("a..b"));
        assert_eq!(confirmation.report.as_deref(), Some("completed safely"));
        assert_eq!(confirmation.revision.as_ref().len(), 64);
        assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Active);
        assert_eq!(store.tasks("foo-bar")[0].completed_at, None);
        assert_eq!(store.tasks("foo-bar")[0].commits, None);
        assert_eq!(
            store.tasks("foo-bar")[0].body,
            "## Goals\n\n- ship the work"
        );
        assert_eq!(store.entries("foo-bar")[0].state, IndexEntryState::Open);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn reopen_rejects_a_task_edited_after_preflight_without_mutation(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let done = IndexEntryState::Done(Some(task_timestamp("2026-01-02T12:34:56Z")));
        let store = staged(TaskStatus::Done, vec![entry(done.clone())]);
        let mut confirmation = EditThenAccept {
            store: store.clone(),
            project: foo(),
            id: TaskId::try_new("FOO-0001").unwrap(),
        };

        let error = reopen_task::execute(&command("FOO-0001"), &store, &pool, &mut confirmation)
            .await
            .unwrap_err();

        assert!(matches!(error, super::ReopenTaskError::Revision(_)));
        assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Done);
        assert!(
            store.tasks("foo-bar")[0]
                .source
                .ends_with("external edit\n")
        );
        assert_eq!(store.entries("foo-bar"), vec![entry(done)]);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn reopen_preserves_absent_index_entry(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(TaskStatus::Done, Vec::new());
        let mut confirmation = TestConfirmation::accepting();

        let out = reopen_task::execute(&command("FOO-0001"), &store, &pool, &mut confirmation)
            .await
            .unwrap();

        assert_eq!(out, ReopenTaskOutcome::Reopened);
        assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Active);
        assert!(store.entries("foo-bar").is_empty());
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn reopen_already_active_is_idempotent_skip(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(TaskStatus::Active, vec![entry(IndexEntryState::Open)]);
        let mut confirmation = TestConfirmation::accepting();

        let out = reopen_task::execute(&command("FOO-0001"), &store, &pool, &mut confirmation)
            .await
            .unwrap();

        assert_eq!(out, ReopenTaskOutcome::AlreadyActive);
        assert!(confirmation.recorded().is_empty());
        assert_eq!(store.tasks("foo-bar")[0].commits.as_deref(), Some("a..b"));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn declined_reopen_preserves_every_completion_artifact(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(
            TaskStatus::Done,
            vec![entry(IndexEntryState::Done(Some(task_timestamp(
                "2026-01-02T12:34:56Z",
            ))))],
        );
        let tasks_before = store.tasks("foo-bar");
        let entries_before = store.entries("foo-bar");
        let mut confirmation = TestConfirmation::declining();

        let outcome = reopen_task::execute(&command("FOO-0001"), &store, &pool, &mut confirmation)
            .await
            .unwrap();

        assert_eq!(outcome, ReopenTaskOutcome::Aborted);
        assert_eq!(store.tasks("foo-bar"), tasks_before);
        assert_eq!(store.entries("foo-bar"), entries_before);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn reopen_reports_an_unknown_project_id(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(TaskStatus::Done, Vec::new());
        let mut confirmation = TestConfirmation::accepting();

        let error = reopen_task::execute(&command("XYZ-0001"), &store, &pool, &mut confirmation)
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown project ID `XYZ` for task XYZ-0001"
        );
    }
}
