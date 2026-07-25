use clap::Args;
use pwf_application::pending_work::{
    ProjectRegistry,
    session::{
        Agent, AgentProbe, DispatchMode, LaunchDirectives, PlanSessionIntent,
        dispatch::{self, DispatchSession, DispatchSessionOk},
        plan::{self, PlanSession, PlanSessionOk},
    },
};
use pwf_infra::{
    obsidian::ObsidianStore,
    session::{
        ClaudeHarness, CodexHarness, InlineHarness, LocalRepositoryClient, TomlModelTierCatalog,
        ZellijHarness,
    },
};

use super::{
    common::{AgentChoice, CommonArguments, Identifier, PendingWorkError},
    render::{
        render_dispatch, render_dry_run, render_session_aborted, render_session_confirmation,
    },
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
    /// Run the agent inline in the current terminal instead of a zellij tab.
    #[arg(long = "inline", short = 'i')]
    pub(crate) inline: bool,
    /// Tell the dispatched agent to isolate its work in a git worktree named after the item
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
    /// Appends content into the item's body before dispatch, growing an existing
    /// section or creating a missing one.
    #[arg(short = 'a', long)]
    pub(crate) append: Option<String>,
    /// Show the exact launch command without editing the task or starting anything.
    #[arg(long, visible_alias = "dry", conflicts_with = "append")]
    pub(crate) dry_run: bool,
    /// Model override forwarded to the selected agent. Wins over effort-tier resolution.
    ///
    /// Use `default` or omit the flag to leave selection to effort-tier policy and provider
    /// configuration.
    #[arg(long, short = 'm')]
    pub(crate) model: Option<String>,
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

pub(super) fn run(
    arguments: &Arguments,
    console: Console,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
) -> Result<String, PendingWorkError> {
    let agent = Agent::from(arguments.agent);

    let request = PlanSession {
        id: arguments.identifier.required("session")?,
        intent: if arguments.dry_run {
            PlanSessionIntent::DryRun
        } else {
            PlanSessionIntent::Dispatch {
                append: arguments.append.clone(),
            }
        },
        mode: if arguments.inline {
            DispatchMode::Inline
        } else {
            DispatchMode::Multiplexer
        },
        directives: LaunchDirectives {
            worktree: arguments.worktree,
            autonomous: arguments.autonomous,
        },
        agent,
        model_override: arguments.model.clone().into(),
    };
    let planned = plan::execute(
        &request,
        store,
        projects,
        &TomlModelTierCatalog,
        &LocalRepositoryClient,
        &ClaudeHarness,
        &CodexHarness,
        &ZellijHarness,
    )?;
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
    let outcome = dispatch::execute(
        DispatchSession::new(planned),
        store,
        &ClaudeHarness,
        &CodexHarness,
        &InlineHarness,
        &ZellijHarness,
    )?;
    Ok(render(&outcome, arguments, console))
}

fn render_probe(probe: &AgentProbe) {
    if !probe.available {
        eprintln!(
            "note: {} not found on PATH from here; the agent will surface the error if it can't run.",
            probe.binary
        );
    }
}

fn render(outcome: &DispatchSessionOk, arguments: &Arguments, console: Console) -> String {
    render_dispatch(
        outcome,
        console.color_with(match arguments.color {
            ColorChoice::Auto => None,
            ColorChoice::Always => Some(true),
            ColorChoice::Never => Some(false),
        }),
    )
}
