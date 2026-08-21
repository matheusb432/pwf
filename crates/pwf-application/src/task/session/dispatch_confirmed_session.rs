//! Plans, confirms, and dispatches one session as a single application operation.

use pwf_models::project::HomeDirectory;

use super::{
    dispatch_session::{self, DispatchSessionError},
    plan_session::{self, PlanSessionError, SessionPlanningClients},
};
use crate::{
    contract::task::session::{DispatchSession, DispatchedSession, PlanSession, PlannedSession},
    ports::{
        agent::AgentClient, project_directory::ProjectDirectoryClient,
        project_note::ProjectNoteStore, session::SessionClient,
        session_confirmation::SessionConfirmationClient, task_record::TaskStore,
    },
};

#[derive(Debug, thiserror::Error)]
pub enum DispatchConfirmedSessionError {
    #[error(transparent)]
    Plan(#[from] PlanSessionError),
    #[error(transparent)]
    Dispatch(#[from] DispatchSessionError),
    #[error("dispatch confirmation requires a dispatch plan")]
    DryRunPlan,
}

#[cqrsy::command]
pub async fn execute(
    command: &PlanSession,
    store: &(impl TaskStore + ProjectNoteStore),
    pool: &sqlx::SqlitePool,
    home: &HomeDirectory,
    clients: &SessionPlanningClients<
        impl AgentClient,
        impl ProjectDirectoryClient,
        impl SessionClient,
    >,
    confirmation: &(impl SessionConfirmationClient + Send + Sync + 'static),
) -> Result<DispatchedSession, DispatchConfirmedSessionError> {
    let prepared = match plan_session::execute(command, store, pool, home, clients).await? {
        PlannedSession::Dispatch(prepared) => prepared,
        PlannedSession::DryRun(_) => return Err(DispatchConfirmedSessionError::DryRunPlan),
    };
    if !confirmation.confirm(&prepared).await {
        return Ok(DispatchedSession::Aborted {
            task_id: prepared.confirmation.task_id.clone(),
        });
    }
    dispatch_session::execute(
        DispatchSession { prepared },
        &clients.agent,
        &clients.session,
    )
    .map_err(Into::into)
}
