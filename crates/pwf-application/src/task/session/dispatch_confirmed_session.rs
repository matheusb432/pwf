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
    project_note::ProjectNoteStore,
    session::SessionClient,
    task_record::TaskStore,
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
    dispatch_session::execute(prepared, &clients.agent, &clients.session).map_err(Into::into)
}
