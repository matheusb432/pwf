use clap::Args;
use pwf_application::pending_work::{
    ProjectRegistry,
    session::{
        Agent,
        verify::{self, VerifySession},
    },
};
use pwf_infra::{
    obsidian::ObsidianStore,
    session::{AgentProbe, ClaudeHarness, CodexHarness, TomlModelTierCatalog, render_argv},
};

use super::{
    common::{AgentChoice, CommonArguments, Identifier, PendingWorkError},
    render::render_verify,
};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Which agent to probe (claude default).
    #[arg(long = "agent", short = 'a', value_enum, default_value_t = AgentChoice::Claude)]
    pub(crate) agent: AgentChoice,
    /// Model override forwarded to the selected agent. Wins over effort-tier resolution.
    ///
    /// Use `default` or omit the flag to leave selection to effort-tier policy and provider
    /// configuration.
    #[arg(long, short = 'm')]
    pub(crate) model: Option<String>,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

pub(in crate::engines::pending_work) struct VerifySessionOk {
    pub task_id: Option<String>,
    pub probe: AgentProbe,
    pub launchable: bool,
    pub issues: Vec<String>,
    pub command_preview: String,
}

pub(super) fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
) -> Result<String, PendingWorkError> {
    let agent = Agent::from(arguments.agent);
    let probe = match agent {
        Agent::Claude => ClaudeHarness::probe(),
        Agent::Codex => CodexHarness::probe(),
    };
    let verification = verify::execute(
        VerifySession {
            id: arguments.identifier.raw().map(str::to_string),
            agent,
            model_override: arguments.model.clone().into(),
        },
        store,
        projects,
        &TomlModelTierCatalog,
    )?;
    let command_preview = match verification.launch.as_ref() {
        Some(launch) => match agent {
            Agent::Claude => render_argv(&ClaudeHarness::preview(launch)),
            Agent::Codex => render_argv(&CodexHarness::preview(launch)),
        },
        None => probe.binary.clone(),
    };
    Ok(render_verify(&VerifySessionOk {
        task_id: verification.task_id,
        probe,
        launchable: verification.launchable,
        issues: verification.issues,
        command_preview,
    }))
}
