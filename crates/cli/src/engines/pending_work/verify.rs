use clap::Args;
use pwf_application::pending_work::{ProjectRegistry, session::verify::VerifySession};
use pwf_domain::pending_work::ProjectName;
use pwf_infra::{
    obsidian::ObsidianStore,
    session::{ProcessSessionRuntime, TomlModelTierCatalog},
};

use super::{
    common::{AgentChoice, CommonArguments, Identifier, PendingWorkError, load_configuration},
    render::render_verify,
};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Which agent to probe (claude default).
    #[arg(long = "agent", short = 'a', value_enum, default_value_t = AgentChoice::Claude)]
    pub(crate) agent: AgentChoice,
    /// Explicit model override, forwarded verbatim to the agent's `--model` flag
    /// (no validation — wins over any effort-tier resolution).
    #[arg(long, short = 'm')]
    pub(crate) model: Option<String>,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

pub(super) fn run(arguments: &Arguments) -> Result<String, PendingWorkError> {
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
    let request = VerifySession {
        id: arguments.identifier.canonical(),
        agent: arguments.agent.into(),
        model_override: arguments.model.clone(),
    };
    let outcome = pwf_application::pending_work::session::verify::execute(
        request,
        &store,
        &projects,
        &TomlModelTierCatalog,
        &ProcessSessionRuntime,
    )?;
    Ok(render_verify(&outcome))
}
