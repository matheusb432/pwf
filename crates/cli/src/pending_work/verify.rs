use clap::Args;
use pwf_application::pending_work::{
    ProjectRegistry,
    session::verify_session::{self, VerifySession},
};
use pwf_infra::{
    obsidian::ObsidianStore,
    session::{ClaudeHarness, CodexHarness, TomlModelTierCatalog},
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

pub(super) fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
) -> Result<String, PendingWorkError> {
    let verification = verify_session::execute(
        VerifySession {
            id: arguments.identifier.raw().map(str::to_string),
            agent: arguments.agent.into(),
            model_override: arguments.model.clone().into(),
        },
        store,
        projects,
        store,
        &TomlModelTierCatalog,
        &ClaudeHarness,
        &CodexHarness,
    )?;
    Ok(render_verify(&verification))
}
