use clap::Args;
use pwf_application::pending_work::{
    ProjectRegistry,
    session::{
        ConfirmationPolicy, DispatchConfirmation, DispatchMode, LaunchDirectives,
        SessionInteraction, dispatch::DispatchSession,
    },
};
use pwf_domain::pending_work::ProjectName;
use pwf_infra::{
    obsidian::ObsidianStore,
    session::{ProcessSessionRuntime, TomlModelTierCatalog},
};

use super::{
    common::{AgentChoice, CommonArguments, Identifier, PendingWorkError, load_configuration},
    render::{render_dispatch, render_session_confirmation},
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
    /// Explicit model override, forwarded verbatim to the agent's `--model` flag
    /// (no validation — wins over any effort-tier resolution).
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

#[derive(Debug, Clone, Copy)]
struct CliSessionInteraction {
    console: Console,
}

impl SessionInteraction for CliSessionInteraction {
    fn warn_agent_missing(&self, binary: &str) {
        eprintln!(
            "note: {binary} not found on PATH from here; the agent will surface the error if it can't run."
        );
    }

    fn confirm(&self, context: &DispatchConfirmation) -> bool {
        matches!(
            self.console
                .confirm(&render_session_confirmation(context), DefaultAnswer::Yes),
            Confirmation::Accepted | Confirmation::NonInteractive
        )
    }

    fn inline_starting(&self, task_id: &str, repository: &str) {
        eprintln!("running {task_id} inline in {repository}…");
    }
}

pub(super) fn run(arguments: &Arguments, console: Console) -> Result<String, PendingWorkError> {
    let configuration = load_configuration(&arguments.common)?;
    let projects = ProjectRegistry::new(configuration.projects.iter().map(|(name, repository)| {
        (
            ProjectName::try_new(name).expect("configured project is non-empty"),
            Some(repository.clone()),
            configuration
                .prefixes
                .get(name)
                .map(|prefix| prefix.to_ascii_uppercase()),
        )
    }));
    let store = ObsidianStore::new(configuration);
    let request = DispatchSession {
        id: arguments.identifier.required("session")?,
        append: arguments.append.clone(),
        mode: if arguments.inline {
            DispatchMode::Inline
        } else {
            DispatchMode::Multiplexer
        },
        directives: LaunchDirectives {
            worktree: arguments.worktree,
            autonomous: arguments.autonomous,
        },
        agent: arguments.agent.into(),
        model_override: arguments.model.clone().into(),
        confirmation: if arguments.assume_yes {
            ConfirmationPolicy::Skip
        } else {
            ConfirmationPolicy::Ask
        },
    };
    let outcome = pwf_application::pending_work::session::dispatch::execute(
        &request,
        &store,
        &projects,
        &TomlModelTierCatalog,
        &ProcessSessionRuntime,
        &CliSessionInteraction { console },
    )?;
    // TODO: make render_dispatch render model
    Ok(render_dispatch(
        &outcome,
        console.color_with(match arguments.color {
            ColorChoice::Auto => None,
            ColorChoice::Always => Some(true),
            ColorChoice::Never => Some(false),
        }),
    ))
}
