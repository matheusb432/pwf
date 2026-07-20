use clap::Args;
use pwf_application::{
    AppDbStore, PendingWorkItem,
    pending_work::{ProjectRegistry, UpdatedItem, update::UpdatePendingWorkItem},
};
use pwf_domain::pending_work::ProjectName;
use pwf_infra::obsidian::ObsidianStore;

use super::common::{CommonArguments, Identifier};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    #[arg(long)]
    pub(crate) prompt: Option<String>,
    /// Replacement title. YAML-breaking characters (e.g. a colon before a
    /// space) are normalized with a stderr notice.
    #[arg(long)]
    pub(crate) title: Option<String>,
    /// Prereq item id to append (repeat or comma-separate); dedups.
    #[arg(long)]
    pub(crate) prereq: Vec<String>,
    /// Clear all prereqs on the item.
    #[arg(long, conflicts_with = "prereq")]
    pub(crate) clear_prereq: bool,
    /// Discovery tag; repeat or comma-separate for several. Input accepts `snake_case` or
    /// kebab-case.
    #[arg(long, allow_hyphen_values = true)]
    pub(crate) tag: Vec<String>,
    /// Remove all tags before applying any supplied `--tag` values.
    #[arg(long)]
    pub(crate) tags_clear: bool,
    /// Overwrite the `commits:` provenance range(s) (repeat or comma-separate);
    /// works on closed done/cancelled items too.
    #[arg(long)]
    pub(crate) commits: Vec<String>,
    /// Append a free-form, multi-line Markdown closeout report to the body
    /// verbatim, under `### Report` — never reruns title/Goals regeneration, so
    /// it is safe on closed done/cancelled items.
    #[arg(long)]
    pub(crate) append_report: Option<String>,
    /// Splice rich lane-syntax bullets (same syntax as `add`'s prompt) into the
    /// body's Goals/Context/Constraints/Done When sections, growing an existing
    /// section or creating a missing one; open items only.
    #[arg(short = 'a', long, conflicts_with = "prompt")]
    pub(crate) append: Option<String>,
    /// Set (or overwrite) the item's effort/complexity tier (1=easy .. 4=xhard).
    /// Optional; open items only, same rule as title/body/prereq edits.
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=4))]
    pub(crate) effort: Option<u8>,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

use super::{
    common::{PendingWorkError, load_configuration},
    render::{TITLE_NORMALIZED_NOTICE, color_enabled_auto, render_updated},
};

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
    let updated = run_update(&store, &projects, arguments)?;
    Ok(render_updated(&updated, color_enabled_auto()))
}

pub(in crate::engines::pending_work) fn run_update(
    store: &impl AppDbStore<PendingWorkItem>,
    projects: &ProjectRegistry,
    args: &Arguments,
) -> Result<UpdatedItem, PendingWorkError> {
    let id = args.identifier.required("update")?;
    let updated = pwf_application::pending_work::update::execute(
        UpdatePendingWorkItem {
            id,
            prompt: args.prompt.clone(),
            title: args.title.clone(),
            append: args.append.clone(),
            prereq: args.prereq.clone(),
            clear_prereq: args.clear_prereq,
            commits: args.commits.clone(),
            append_report: args.append_report.clone(),
            effort: args.effort,
            tags: args.tag.clone(),
            tags_clear: args.tags_clear,
        },
        store,
        projects,
    )
    .map_err(|error| PendingWorkError::ApplicationWrite(error.to_string()))?;
    if updated.title_normalized() {
        eprintln!("{TITLE_NORMALIZED_NOTICE}");
    }
    Ok(updated)
}
