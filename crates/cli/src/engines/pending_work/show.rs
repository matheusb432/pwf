use clap::Args;
use pwf_application::{
    AppDbStore, NoteMarkdownSource, PendingWorkItem,
    pending_work::{ProjectRegistry, ShowOutput, show::ShowPendingWorkItem},
};
use pwf_domain::pending_work::ProjectName;
use pwf_infra::obsidian::ObsidianStore;

use super::common::{CommonArguments, Identifier};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Print the item's note path instead of the note markdown.
    #[arg(long)]
    pub(crate) path: bool,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

use super::common::{PendingWorkError, load_configuration};
use crate::config::Config;

pub(super) fn run(arguments: &Arguments) -> Result<String, PendingWorkError> {
    let configuration = load_configuration(&arguments.common)?;
    let store = ObsidianStore::new(configuration.clone());
    run_show(&configuration, &store, arguments)
}

/// Preserves the caller's spelling when canonicalization resolves to the same item.
fn lookup_id(arguments: &Arguments) -> Result<String, PendingWorkError> {
    let canonical = arguments.identifier.required("show")?;
    Ok(match arguments.identifier.raw() {
        Some(raw) if pwf_domain::pending_work::canonical_pending_id(raw) == canonical => {
            raw.to_string()
        }
        _ => canonical,
    })
}

/// Returns the complete Markdown for an item regardless of status;
/// `--path` returns the note path instead.
pub(in crate::engines::pending_work) fn run_show<S>(
    cfg: &Config,
    store: &S,
    args: &Arguments,
) -> Result<String, PendingWorkError>
where
    S: AppDbStore<PendingWorkItem> + NoteMarkdownSource,
{
    let projects = ProjectRegistry::new(cfg.projects.iter().map(|(name, repository)| {
        (
            ProjectName::try_new(name).expect("configured project is non-empty"),
            Some(repository.clone()),
            cfg.prefixes
                .get(name)
                .map(|prefix| prefix.to_ascii_uppercase()),
        )
    }));
    let id = lookup_id(args)?;
    let output = if args.path {
        ShowOutput::Path
    } else {
        ShowOutput::Markdown
    };
    pwf_application::pending_work::show::execute(
        &ShowPendingWorkItem { id, output },
        store,
        &projects,
        store,
    )
    .map_err(|error| PendingWorkError::ApplicationRead(error.to_string()))
}
