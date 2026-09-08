//! Plans, confirms, and dispatches one session as a single application operation.

use pwf_models::project::HomeDirectory;
use pwf_wire::task::session::{
    DispatchedSession, PlanSession, PlannedSession, PreparedSessionDispatch,
};

use super::{
    dispatch_session::{self, DispatchSessionError},
    plan_session::{self, PlanSessionError, SessionPlanningClients},
};
use crate::ports::{
    agent::AgentClient,
    confirmation::{ConfirmationClient, ConfirmationClientError},
    project_directory::ProjectDirectoryClient,
    task_vault::{ExpectedTaskRevision, TaskMutationError, TaskVault},
};

#[derive(Debug, thiserror::Error)]
pub enum DispatchConfirmedSessionError {
    #[error(transparent)]
    Plan(#[from] PlanSessionError),
    #[error(transparent)]
    Dispatch(#[from] DispatchSessionError),
    #[error(transparent)]
    Confirmation(#[from] ConfirmationClientError),
    #[error("dispatch confirmation requires a dispatch plan")]
    DryRunPlan,
    #[error(transparent)]
    Mutation(#[from] TaskMutationError<anyhow::Error>),
}

#[cqrsy::command]
pub async fn execute(
    command: &PlanSession,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
    home: &HomeDirectory,
    clients: &SessionPlanningClients<impl AgentClient, impl ProjectDirectoryClient>,
    confirmation: &mut dyn ConfirmationClient<Confirmation = PreparedSessionDispatch>,
) -> Result<DispatchedSession, DispatchConfirmedSessionError> {
    let prepared = match plan_session::execute(command, store, pool, home, clients).await? {
        PlannedSession::Dispatch(prepared) => prepared,
        PlannedSession::DryRun(_) => return Err(DispatchConfirmedSessionError::DryRunPlan),
    };
    if !confirmation.confirm(&prepared).await? {
        return Ok(DispatchedSession::Aborted {
            task_ids: prepared.confirmation.task_ids.clone(),
        });
    }
    let expected = prepared
        .task_revisions
        .iter()
        .map(|task| ExpectedTaskRevision {
            id: task.task_id.clone(),
            revision: task.revision.clone(),
        })
        .collect();
    crate::task::commit_task_writes(store, &prepared.project, expected, Vec::new())?;
    dispatch_session::execute(prepared, &clients.agent).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use pwf_models::{
        project::{HomeDirectory, Project},
        session::{Agent, AgentModel, SessionEffort, SessionTaskIds},
        task::TaskId,
    };
    use pwf_wire::task::session::{
        AgentAvailability, AgentLaunch, AgentProbe, PlanSession, PlanSessionIntent,
        PreparedSessionDispatch,
    };

    use super::{DispatchConfirmedSessionError, SessionPlanningClients};
    use crate::{
        ports::{
            agent::{AgentClient, PreparedAgentLaunch},
            confirmation::{ConfirmationClient, ConfirmationClientError},
            project_directory::ProjectDirectoryClient,
            task_vault::TaskMutationError,
        },
        task::session::dispatch_confirmed_session,
        testing::{InMemoryStore, insert_project, project, task_record},
    };

    #[derive(Clone)]
    struct CountingAgent {
        preparations: Arc<AtomicUsize>,
    }

    impl AgentClient for CountingAgent {
        type PreparationError = Infallible;

        fn probe(&self, agent: Agent) -> AgentProbe {
            AgentProbe {
                agent,
                availability: AgentAvailability::Available,
            }
        }

        fn preview(&self, _: &AgentLaunch) -> Vec<String> {
            vec!["codex".to_string()]
        }

        fn prepare(&self, _: &AgentLaunch) -> Result<PreparedAgentLaunch, Self::PreparationError> {
            self.preparations.fetch_add(1, Ordering::SeqCst);
            Ok(PreparedAgentLaunch::Process {
                arguments: vec!["codex".to_string()],
            })
        }
    }

    #[derive(Clone, Copy)]
    struct ExistingProjectDirectory;

    impl ProjectDirectoryClient for ExistingProjectDirectory {
        fn canonicalize(&self, path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
            Ok(path.to_path_buf())
        }

        fn is_directory(&self, _: &std::path::Path) -> bool {
            true
        }
    }

    struct EditThenAccept {
        store: InMemoryStore,
        project: Project,
        id: TaskId,
    }

    impl ConfirmationClient for EditThenAccept {
        type Confirmation = PreparedSessionDispatch;

        fn confirm<'a>(
            &'a mut self,
            _confirmation: &'a PreparedSessionDispatch,
        ) -> futures::future::BoxFuture<'a, Result<bool, ConfirmationClientError>> {
            self.store.externally_edit_task(&self.project, &self.id);
            Box::pin(futures::future::ready(Ok(true)))
        }
    }

    fn command(ids: impl IntoIterator<Item = &'static str>) -> PlanSession {
        PlanSession {
            task_ids: SessionTaskIds::try_new(
                ids.into_iter()
                    .map(|id| TaskId::try_new(id).unwrap())
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
            intent: PlanSessionIntent::Dispatch,
            pushed_prompt: None,
            agent: Agent::Codex,
            model_override: AgentModel::from(None),
            effort: SessionEffort::Max,
        }
    }

    async fn assert_stale_dispatch(
        pool: &sqlx::SqlitePool,
        ids: impl IntoIterator<Item = &'static str>,
        changed: &str,
    ) {
        insert_project(pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
        let ids = ids.into_iter().collect::<Vec<_>>();
        let records = ids.iter().copied().map(task_record).collect::<Vec<_>>();
        let command = command(ids.iter().copied());
        let store = InMemoryStore::default().with_project("foo", records);
        let preparations = Arc::new(AtomicUsize::new(0));
        let clients = SessionPlanningClients::new(
            CountingAgent {
                preparations: preparations.clone(),
            },
            ExistingProjectDirectory,
        );
        let mut confirmation = EditThenAccept {
            store: store.clone(),
            project: project("FOO", "foo"),
            id: TaskId::try_new(changed).unwrap(),
        };

        let error = dispatch_confirmed_session::execute(
            &command,
            &store,
            pool,
            &HomeDirectory::new("/home/dev".into()),
            &clients,
            &mut confirmation,
        )
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            DispatchConfirmedSessionError::Mutation(TaskMutationError::StaleTask { .. })
        ));
        assert_eq!(preparations.load(Ordering::SeqCst), 0);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn singleton_dispatch_rejects_a_task_edited_after_preflight(pool: sqlx::SqlitePool) {
        assert_stale_dispatch(&pool, ["FOO-0001"], "FOO-0001").await;
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn multi_task_dispatch_rejects_all_dispatch_when_one_task_changes(
        pool: sqlx::SqlitePool,
    ) {
        assert_stale_dispatch(&pool, ["FOO-0001", "FOO-0002"], "FOO-0002").await;
    }
}
