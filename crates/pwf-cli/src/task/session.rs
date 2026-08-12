use std::path::PathBuf;

use clap::Args;
use pwf_application::task::session::{
    PlanSessionIntent,
    dispatch_session::{self, DispatchSession},
    plan_session::{self, PlanSession},
};
use pwf_infra::{
    obsidian::ObsidianStore,
    session::{AgentHarness, InlineHarness, LocalProjectDirectoryClient, TmuxHarness, render_argv},
};
use pwf_models::session::{Agent, DispatchMode, LaunchDirectives, PushedPrompt, SessionEffort};
use pwf_wire::task::session::{AgentProbe, PlannedSession};

use super::{
    AgentChoice, Identifier, TaskError,
    render::{
        render_dispatch, render_dry_run, render_session_aborted, render_session_confirmation,
    },
};
use crate::{confirm::Confirmation, console::Console};

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
    /// Skip the [Y/n] dispatch confirmation
    #[arg(long = "yes", short = 'y')]
    assume_yes: bool,
}

impl ConfirmationArguments {
    fn is_required(&self) -> bool {
        !self.assume_yes
    }
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
    home: &PathBuf,
) -> Result<String, TaskError> {
    let agent = Agent::from(arguments.agent);

    let request = PlanSession {
        task_id: arguments.identifier.required("session")?,
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

    if arguments.confirmation.is_required()
        && matches!(
            console.confirm(&render_session_confirmation(&planned.confirmation)),
            Confirmation::Declined
        )
    {
        return Ok(render_session_aborted(&planned.confirmation.task_id));
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
    )?;
    Ok(render_dispatch(
        &outcome,
        console.color_with(match arguments.color {
            ColorChoice::Auto => None,
            ColorChoice::Always => Some(true),
            ColorChoice::Never => Some(false),
        }),
    ))
}

fn map_plan_error(error: plan_session::PlanSessionError) -> TaskError {
    match error {
        plan_session::PlanSessionError::MultiplexerSessionMissing {
            session,
            start_command_argv,
        } => TaskError::TmuxSessionMissing {
            session,
            start_command: render_argv(&start_command_argv),
        },
        error => TaskError::SessionPlan(error),
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
