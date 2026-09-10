use std::{
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use pwf_application::{
    ports::{
        agent::{AgentClient, PreparedAgentLaunch},
        confirmation::{ConfirmationClient, ConfirmationClientError},
        task_vault::TaskMutationError,
    },
    task::session::{
        dispatch_confirmed_session, dispatch_confirmed_session::DispatchConfirmedSessionError,
        plan_session::SessionPlanningClients,
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

use crate::support::{
    ExistingProjectDirectory, InMemoryStore, insert_project, project, task_record,
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

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn singleton_dispatch_rejects_a_task_edited_after_preflight(pool: sqlx::SqlitePool) {
    assert_stale_dispatch(&pool, ["FOO-0001"], "FOO-0001").await;
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn multi_task_dispatch_rejects_all_dispatch_when_one_task_changes(pool: sqlx::SqlitePool) {
    assert_stale_dispatch(&pool, ["FOO-0001", "FOO-0002"], "FOO-0002").await;
}
