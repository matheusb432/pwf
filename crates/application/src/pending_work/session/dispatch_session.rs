//! Dispatches one confirmed pending-work session.

use std::error::Error;

use thiserror::Error;

use super::{
    Agent, ClaudeSessionClient, CodexSessionClient, DispatchMode, DispatchTarget,
    InlineSessionClient, TmuxSessionClient, plan_session::PreparedSessionDispatch, task_content,
};
use crate::{
    AppRecordStore, NoteMarkdownSource, PendingWorkRecord,
    pending_work::{
        project_registry::ProjectRegistry,
        show_pending_work_item::ShowPendingWorkError,
        update_pending_work_item::{self, UpdatePendingWorkError},
    },
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
    WindowOpened {
        target: DispatchTarget,
        agent: Agent,
        repository: String,
    },
}

#[derive(Debug, Error)]
pub enum DispatchSessionError {
    #[error(transparent)]
    Update(#[from] UpdatePendingWorkError),
    #[error(transparent)]
    Show(#[from] ShowPendingWorkError),
    #[error("Failed to run agent inline: {message}")]
    InlineFailed { message: String },
    #[error("Failed to open tmux window '{window}' in session '{session}': {message}")]
    WindowOpen {
        session: String,
        window: String,
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
    store: &(impl AppRecordStore<PendingWorkRecord> + NoteMarkdownSource),
    projects: &ProjectRegistry,
    claude: &impl ClaudeSessionClient,
    codex: &impl CodexSessionClient,
    inline: &impl InlineSessionClient,
    tmux: &impl TmuxSessionClient,
) -> Result<DispatchSessionOk, DispatchSessionError> {
    let PreparedSessionDispatch {
        mut plan,
        confirmation,
        prepared_update,
        ..
    } = command.prepared;
    if let Some(prepared_update) = prepared_update {
        update_pending_work_item::persist(prepared_update, store)?;
        let task_content = task_content::load(&plan.launch.task_id, store, projects, store)?;
        plan.launch.prompt = super::launch::launch_prompt(
            &task_content,
            &plan.launch.task_id,
            confirmation.directives,
        );
    }

    match plan.launch.agent {
        Agent::Claude => {
            let argv = claude.prepare(&plan.launch);
            dispatch_host(&argv, &plan, inline, tmux)
        }
        Agent::Codex => {
            let prepared = codex.prepare(&plan.launch).map_err(|source| {
                DispatchSessionError::CodexPreparation {
                    source: Box::new(source),
                }
            })?;
            dispatch_host(prepared.argv(), &plan, inline, tmux).map_err(|error| {
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
    tmux: &impl TmuxSessionClient,
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
        DispatchMode::Multiplexer => dispatch_multiplexer(argv, plan, tmux),
    }
}

fn dispatch_multiplexer(
    argv: &[String],
    plan: &super::SessionPlan,
    tmux: &impl TmuxSessionClient,
) -> Result<DispatchSessionOk, DispatchSessionError> {
    tmux.open_window(
        &plan.target.session,
        &plan.launch.repository,
        &plan.target.window,
        argv,
    )
    .map_err(|message| DispatchSessionError::WindowOpen {
        session: plan.target.session.clone(),
        window: plan.target.window.clone(),
        message,
    })?;
    Ok(DispatchSessionOk::WindowOpened {
        target: plan.target.clone(),
        agent: plan.launch.agent,
        repository: plan.launch.repository.clone(),
    })
}
