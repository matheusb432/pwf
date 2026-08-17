use clap::Args;
use pwf_application::task::session::{
    dispatch_session::{self, DispatchSessionError},
    plan_session::{self, PlanSessionError},
};
use pwf_infra::{
    obsidian::ObsidianStore,
    session::{AgentHarness, InlineHarness, LocalProjectDirectoryClient, TmuxHarness, render_argv},
};
use pwf_models::{
    project::HomeDirectory,
    session::{Agent, DispatchMode, LaunchDirectives, PushedPrompt, SessionEffort},
};
use pwf_wire::task::session::{
    AgentProbe, DispatchSession, DispatchSessionApiError, PlanSession, PlanSessionApiError,
    PlanSessionIntent, PlannedSession,
};

use super::{
    AgentChoice, Identifier,
    render::{
        render_dispatch, render_dry_run, render_session_aborted, render_session_confirmation,
    },
};
use crate::{
    confirmation::{ConfirmationAnswer, ConfirmationMode, prompt_error},
    console::Console,
};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Color policy for the dispatch output
    #[arg(long, value_enum, default_value_t = ColorChoice::Auto)]
    pub(crate) color: ColorChoice,
    #[command(flatten)]
    confirmation: ConfirmationArguments,
    #[command(flatten)]
    execution: ExecutionArguments,
    #[command(flatten)]
    launch: LaunchDirectiveArguments,
    /// Which agent to dispatch
    #[arg(long = "agent", value_enum, default_value_t = AgentChoice::default())]
    pub(crate) agent: AgentChoice,
    /// Prefix text pushed to the agent prompt
    #[arg(short = 'p', long = "push-prompt", value_name = "TEXT")]
    pub(crate) pushed_prompt: Option<PushedPrompt>,
    /// Model override forwarded to the selected agent Wins over effort-tier resolution
    ///
    /// Use `default` or omit the flag to leave selection to effort-tier policy and provider
    /// configuration
    #[arg(long, short = 'm')]
    pub(crate) model: Option<String>,
    /// Reasoning effort for the dispatched agent session
    #[arg(long, value_enum, default_value_t = SessionEffortChoice::default())]
    pub(crate) effort: SessionEffortChoice,
}

#[derive(Args, Debug)]
struct ConfirmationArguments {
    /// Skip the dispatch confirmation (assume yes)
    #[arg(long = "yes", short = 'y')]
    assume_yes: bool,
}

#[derive(Args, Debug)]
struct ExecutionArguments {
    /// Run the agent inline in the current terminal
    #[arg(long = "inline", short = 'i')]
    inline: bool,
    /// Show the exact launch command without editing the task or starting anything
    #[arg(long, visible_alias = "dry")]
    dry_run: bool,
}

impl ExecutionArguments {
    fn intent(&self) -> PlanSessionIntent {
        if self.dry_run {
            PlanSessionIntent::DryRun
        } else {
            PlanSessionIntent::Dispatch
        }
    }

    fn mode(&self) -> DispatchMode {
        if self.inline {
            DispatchMode::Inline
        } else {
            DispatchMode::Multiplexer
        }
    }
}

#[derive(Args, Debug)]
struct LaunchDirectiveArguments {
    /// Instructs the agent to work in a git worktree named after the task id
    #[arg(long = "worktree", short = 'w')]
    worktree: bool,
    /// Append an autonomy directive so the agent runs without prompting the user (for
    /// unattended dispatch)
    #[arg(long = "auto")]
    autonomous: bool,
}

impl LaunchDirectiveArguments {
    fn directives(&self) -> LaunchDirectives {
        LaunchDirectives {
            worktree: self.worktree,
            autonomous: self.autonomous,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SessionEffortChoice {
    Low,
    Medium,
    #[default]
    High,
    #[value(name = "xhigh")]
    XHigh,
    Max,
}

impl From<SessionEffortChoice> for SessionEffort {
    fn from(choice: SessionEffortChoice) -> Self {
        match choice {
            SessionEffortChoice::Low => Self::Low,
            SessionEffortChoice::Medium => Self::Medium,
            SessionEffortChoice::High => Self::High,
            SessionEffortChoice::XHigh => Self::XHigh,
            SessionEffortChoice::Max => Self::Max,
        }
    }
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
    home: &HomeDirectory,
) -> anyhow::Result<String> {
    let agent = Agent::from(arguments.agent);
    let task_id = arguments
        .identifier
        .required(PlanSessionApiError::MissingId)?;
    let confirmation_mode = if arguments.execution.dry_run {
        ConfirmationMode::AssumeYes
    } else {
        console.confirmation_mode(arguments.confirmation.assume_yes)?
    };

    let request = PlanSession {
        task_id,
        intent: arguments.execution.intent(),
        pushed_prompt: arguments.pushed_prompt.clone(),
        mode: arguments.execution.mode(),
        directives: arguments.launch.directives(),
        agent,
        model_override: arguments.model.clone().into(),
        effort: arguments.effort.into(),
    };
    let planned = plan_session::execute(
        &request,
        store,
        pool,
        home,
        &AgentHarness,
        &LocalProjectDirectoryClient,
        &TmuxHarness,
    )
    .await
    .map_err(map_plan_error)?;
    let planned = match planned {
        PlannedSession::DryRun(dry_run) => {
            render_probe(&dry_run.probe);
            return Ok(render_dry_run(&dry_run.plan, &dry_run.argv));
        }
        PlannedSession::Dispatch(planned) => planned,
    };
    render_probe(&planned.probe);

    if confirmation_mode == ConfirmationMode::Prompt {
        let answer = console
            .confirm(&render_session_confirmation(&planned.confirmation))
            .map_err(|source| prompt_error("session dispatch", source))?;
        if answer == ConfirmationAnswer::Declined {
            return Ok(render_session_aborted(&planned.confirmation.task_id));
        }
    }

    if planned.plan.mode == DispatchMode::Inline {
        eprintln!(
            "running {} inline in {}...",
            planned.plan.launch.task_id, planned.plan.launch.project_path
        );
    }
    let outcome = dispatch_session::execute(
        DispatchSession { prepared: planned },
        &AgentHarness,
        &InlineHarness,
        &TmuxHarness,
    )
    .map_err(map_dispatch_error)?;
    Ok(render_dispatch(
        &outcome,
        console.color_with(match arguments.color {
            ColorChoice::Auto => None,
            ColorChoice::Always => Some(true),
            ColorChoice::Never => Some(false),
        }),
    ))
}

fn map_plan_error(error: PlanSessionError) -> PlanSessionApiError {
    match error {
        PlanSessionError::NotLaunchable { id, launch } => {
            PlanSessionApiError::NotLaunchable { id, launch }
        }
        PlanSessionError::ProjectPathMissing { project_id, path } => {
            PlanSessionApiError::ProjectPathMissing { project_id, path }
        }
        PlanSessionError::MultiplexerNotFound => PlanSessionApiError::MultiplexerNotFound,
        PlanSessionError::MultiplexerSessionMissing {
            session,
            start_command_argv,
        } => PlanSessionApiError::MultiplexerSessionMissing {
            session,
            start_command: render_argv(&start_command_argv),
        },
        PlanSessionError::EmptyAgentCommand => PlanSessionApiError::EmptyAgentCommand,
        error => PlanSessionApiError::Unexpected {
            message: error.to_string(),
        },
    }
}

fn map_dispatch_error(error: DispatchSessionError) -> DispatchSessionApiError {
    match error {
        DispatchSessionError::InlineFailed { source } => DispatchSessionApiError::InlineFailed {
            reason: source.to_string(),
        },
        DispatchSessionError::WindowOpen {
            session,
            window,
            source,
        } => DispatchSessionApiError::WindowOpen {
            session,
            window,
            reason: source.to_string(),
        },
        DispatchSessionError::AgentPreparation { source } => {
            DispatchSessionApiError::AgentPreparation {
                message: source.to_string(),
            }
        }
        DispatchSessionError::NamedThreadBackend { thread_id, source } => {
            DispatchSessionApiError::NamedThreadBackend {
                thread_id,
                reason: source.to_string(),
            }
        }
        DispatchSessionError::EmptyAgentCommand => DispatchSessionApiError::EmptyAgentCommand,
    }
}

fn render_probe(probe: &AgentProbe) {
    if !probe.is_available() {
        eprintln!(
            "note: {} not found on PATH from here; the agent will surface the error if it can't run.",
            probe.binary()
        );
    }
}
