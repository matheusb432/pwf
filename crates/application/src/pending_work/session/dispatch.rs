//! Dispatches one confirmed pending-work session.

use std::error::Error;

use thiserror::Error;

use super::{
    Agent, ClaudeSessionClient, CodexSessionClient, DispatchMode, DispatchTarget,
    InlineSessionClient, ZellijSessionClient, ZellijTabOpenError, plan::PreparedSessionDispatch,
};
use crate::{
    AppDbStore, PendingWorkItem,
    pending_work::update::{self, UpdatePendingWorkError},
};

pub struct DispatchSession {
    prepared: PreparedSessionDispatch,
}

impl DispatchSession {
    #[must_use]
    pub fn new(prepared: PreparedSessionDispatch) -> Self {
        Self { prepared }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchSessionOk {
    Inline {
        task_id: String,
    },
    TabOpened {
        target: DispatchTarget,
        agent: Agent,
        repository: String,
    },
    MultiplexerStartedAndTabOpened {
        target: DispatchTarget,
        agent: Agent,
        repository: String,
    },
}

#[derive(Debug, Error)]
pub enum DispatchSessionError {
    #[error(transparent)]
    Update(#[from] UpdatePendingWorkError),
    #[error("Failed to run agent inline: {message}")]
    InlineFailed { message: String },
    #[error("Failed to dispatch into zellij session '{session}': {message}")]
    SessionEnsureFailed { session: String, message: String },
    #[error("Failed to open zellij tab '{tab}' in session '{session}': {message}")]
    TabOpen {
        session: String,
        tab: String,
        message: String,
    },
    #[error("{source}")]
    CodexPreparation {
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
    #[error(
        "Codex backend failed after naming thread '{thread_id}': {message}. The named thread was left intact."
    )]
    CodexBackend { thread_id: String, message: String },
}

#[cqrsy::command]
pub fn execute(
    command: DispatchSession,
    store: &impl AppDbStore<PendingWorkItem>,
    claude: &impl ClaudeSessionClient,
    codex: &impl CodexSessionClient,
    inline: &impl InlineSessionClient,
    zellij: &impl ZellijSessionClient,
) -> Result<DispatchSessionOk, DispatchSessionError> {
    let PreparedSessionDispatch {
        plan,
        prepared_update,
        ..
    } = command.prepared;
    if let Some(prepared_update) = prepared_update {
        update::persist(prepared_update, store)?;
    }

    match plan.launch.agent {
        Agent::Claude => {
            let argv = claude.prepare(&plan.launch);
            dispatch_host(&argv, &plan, inline, zellij)
        }
        Agent::Codex => {
            let prepared = codex.prepare(&plan.launch).map_err(|source| {
                DispatchSessionError::CodexPreparation {
                    source: Box::new(source),
                }
            })?;
            dispatch_host(prepared.argv(), &plan, inline, zellij).map_err(|error| {
                DispatchSessionError::CodexBackend {
                    thread_id: prepared.thread_id().to_string(),
                    message: error.to_string(),
                }
            })
        }
    }
}

fn dispatch_host(
    argv: &[String],
    plan: &super::SessionPlan,
    inline: &impl InlineSessionClient,
    zellij: &impl ZellijSessionClient,
) -> Result<DispatchSessionOk, DispatchSessionError> {
    match plan.mode {
        DispatchMode::Inline => {
            inline
                .run(argv, &plan.launch.repository)
                .map_err(|message| DispatchSessionError::InlineFailed { message })?;
            Ok(DispatchSessionOk::Inline {
                task_id: plan.launch.task_id.clone(),
            })
        }
        DispatchMode::Multiplexer => dispatch_multiplexer(argv, plan, zellij),
    }
}

fn dispatch_multiplexer(
    argv: &[String],
    plan: &super::SessionPlan,
    zellij: &impl ZellijSessionClient,
) -> Result<DispatchSessionOk, DispatchSessionError> {
    match open_tab(argv, plan, zellij) {
        Ok(()) => Ok(DispatchSessionOk::TabOpened {
            target: plan.target.clone(),
            agent: plan.launch.agent,
            repository: plan.launch.repository.clone(),
        }),
        Err(ZellijTabOpenError::Rejected(message)) => Err(tab_open_error(plan, message)),
        Err(ZellijTabOpenError::SessionNotFound) => {
            zellij
                .ensure_session(&plan.target.session)
                .map_err(|message| DispatchSessionError::SessionEnsureFailed {
                    session: plan.target.session.clone(),
                    message,
                })?;
            match open_tab(argv, plan, zellij) {
                Ok(()) => Ok(DispatchSessionOk::MultiplexerStartedAndTabOpened {
                    target: plan.target.clone(),
                    agent: plan.launch.agent,
                    repository: plan.launch.repository.clone(),
                }),
                Err(ZellijTabOpenError::SessionNotFound) => {
                    Err(tab_open_error(plan, "session not found".to_string()))
                }
                Err(ZellijTabOpenError::Rejected(message)) => Err(tab_open_error(plan, message)),
            }
        }
    }
}

fn open_tab(
    argv: &[String],
    plan: &super::SessionPlan,
    zellij: &impl ZellijSessionClient,
) -> Result<(), ZellijTabOpenError> {
    zellij.open_tab(
        &plan.target.session,
        &plan.launch.repository,
        &plan.target.tab,
        argv,
    )
}

fn tab_open_error(plan: &super::SessionPlan, message: String) -> DispatchSessionError {
    DispatchSessionError::TabOpen {
        session: plan.target.session.clone(),
        tab: plan.target.tab.clone(),
        message,
    }
}
