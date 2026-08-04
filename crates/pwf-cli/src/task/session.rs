use std::path::PathBuf;

use clap::Args;
use pwf_application::task::session::{
    AgentProbe, PlanSessionIntent,
    dispatch_session::{self, DispatchSession},
    plan_session::{self, PlanSession, PlanSessionOk},
};
use pwf_infra::{
    obsidian::ObsidianStore,
    session::{AgentHarness, InlineHarness, LocalRepositoryClient, TmuxHarness, render_argv},
};
use pwf_models::session::{Agent, DispatchMode, LaunchDirectives, PushedPrompt, SessionEffort};

use super::{
    render::{
        render_dispatch, render_dry_run, render_session_aborted, render_session_confirmation,
    },
    shared::{AgentChoice, CommonArguments, Identifier, TaskError},
};
use crate::{
    confirm::{Confirmation, DefaultAnswer},
    console::Console,
};

#[derive(Args, Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "clap mirrors independent command-line switches"
)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Color policy for the dispatch output.
    #[arg(long, value_enum, default_value_t = ColorChoice::Auto)]
    pub(crate) color: ColorChoice,
    /// Skip the [Y/n] dispatch confirmation (assume yes).
    #[arg(long = "yes", short = 'y')]
    pub(crate) assume_yes: bool,
    /// Run the agent inline in the current terminal instead of a tmux window.
    #[arg(long = "inline", short = 'i')]
    pub(crate) inline: bool,
    /// Tell the dispatched agent to isolate its work in a git worktree named after the task
    /// id.
    #[arg(long = "worktree", short = 'w')]
    pub(crate) worktree: bool,
    /// Append an autonomy directive so the agent runs without prompting the user (for
    /// unattended dispatch).
    #[arg(long = "auto")]
    pub(crate) autonomous: bool,
    /// Which agent to dispatch.
    #[arg(long = "agent", value_enum, default_value_t = AgentChoice::default())]
    pub(crate) agent: AgentChoice,
    /// Prefix text pushed to the agent prompt.
    #[arg(short = 'p', long = "push-prompt", value_name = "TEXT")]
    pub(crate) pushed_prompt: Option<PushedPrompt>,
    /// Show the exact launch command without editing the task or starting anything.
    #[arg(long, visible_alias = "dry")]
    pub(crate) dry_run: bool,
    /// Model override forwarded to the selected agent. Wins over effort-tier resolution.
    ///
    /// Use `default` or omit the flag to leave selection to effort-tier policy and provider
    /// configuration.
    #[arg(long, short = 'm')]
    pub(crate) model: Option<String>,
    /// Reasoning effort for the dispatched agent session.
    #[arg(long, value_enum, default_value_t = SessionEffortChoice::default())]
    pub(crate) effort: SessionEffortChoice,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
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

    let request = PlanSession::builder(arguments.identifier.required_task_id("session")?)
        .intent(if arguments.dry_run {
            PlanSessionIntent::DryRun
        } else {
            PlanSessionIntent::Dispatch
        })
        .maybe_pushed_prompt(arguments.pushed_prompt.clone())
        .mode(if arguments.inline {
            DispatchMode::Inline
        } else {
            DispatchMode::Multiplexer
        })
        .directives(LaunchDirectives {
            worktree: arguments.worktree,
            autonomous: arguments.autonomous,
        })
        .agent(agent)
        .model_override(arguments.model.clone().into())
        .effort(arguments.effort.into())
        .build();
    let planned = plan_session::execute(
        &request,
        store,
        pool,
        home,
        &AgentHarness,
        &LocalRepositoryClient,
        &TmuxHarness,
    )
    .await
    .map_err(map_plan_error)?;
    let planned = match planned {
        PlanSessionOk::DryRun(dry_run) => {
            render_probe(dry_run.probe());
            return Ok(render_dry_run(dry_run.plan(), dry_run.argv()));
        }
        PlanSessionOk::Dispatch(planned) => planned,
    };
    render_probe(planned.probe());

    if !arguments.assume_yes
        && matches!(
            console.confirm(
                &render_session_confirmation(planned.confirmation()),
                DefaultAnswer::Yes
            ),
            Confirmation::Declined
        )
    {
        return Ok(render_session_aborted(&planned.confirmation().task_id));
    }

    if planned.plan().mode == DispatchMode::Inline {
        eprintln!(
            "running {} inline in {}...",
            planned.plan().launch.task_id,
            planned.plan().launch.repository
        );
    }
    let outcome = dispatch_session::execute(
        DispatchSession::new(planned),
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
    if !probe.available {
        eprintln!(
            "note: {} not found on PATH from here; the agent will surface the error if it can't run.",
            probe.binary
        );
    }
}
