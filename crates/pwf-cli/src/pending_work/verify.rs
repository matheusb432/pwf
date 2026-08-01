use clap::Args;
use pwf_application::pending_work::session::verify_session::{self, VerifySession};
use pwf_infra::{obsidian::ObsidianStore, session::AgentHarness};

use super::{
    render::render_verify,
    shared::{AgentChoice, CommonArguments, Identifier, PendingWorkError},
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

pub(super) async fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
) -> Result<String, PendingWorkError> {
    let verification = verify_session::execute(
        VerifySession {
            id: arguments.identifier.raw().map(str::to_string),
            agent: arguments.agent.into(),
            model_override: arguments.model.clone().into(),
        },
        store,
        pool,
        &AgentHarness,
    )
    .await?;
    Ok(render_verify(&verification))
}
