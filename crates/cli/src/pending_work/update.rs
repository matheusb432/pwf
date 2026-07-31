use clap::Args;
use pwf_application::{
    pending_work::{
        ProjectRegistry, UpdatePendingWorkItemOk,
        update_pending_work_item::{self, UpdatePendingWorkItem},
    },
    ports::{app_record::AppRecordStore, pending_work_record::PendingWorkRecord},
};
use pwf_infra::obsidian::ObsidianStore;

use super::shared::{CommonArguments, EffortChoice, Identifier, task_title};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    #[arg(long)]
    pub(crate) prompt: Option<String>,
    /// Replacement title. YAML-breaking characters (e.g. a colon before a
    /// space) are normalized with a stderr notice. The normalized title cannot
    /// exceed 200 characters.
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
    /// Append multi-line Markdown under `### Report` verbatim; works on closed
    /// done/cancelled items too.
    #[arg(long)]
    pub(crate) append_report: Option<String>,
    /// Splice lane-syntax bullets (same syntax as `add`'s prompt) into the body's
    /// Goals/Context/Constraints/Done When sections; open items only.
    #[arg(short = 'a', long, conflicts_with = "prompt")]
    pub(crate) append: Option<String>,
    /// Effort tier; open items only.
    #[arg(long, value_enum)]
    pub(crate) effort: Option<EffortChoice>,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

use super::{
    render::{TITLE_NORMALIZED_NOTICE, render_updated},
    shared::PendingWorkError,
};
use crate::console::Console;

pub(super) fn run(
    arguments: &Arguments,
    console: Console,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
) -> Result<String, PendingWorkError> {
    let updated = run_update(store, projects, arguments)?;
    Ok(render_updated(&updated, console.color()))
}

pub(in crate::pending_work) fn run_update(
    store: &impl AppRecordStore<PendingWorkRecord>,
    projects: &ProjectRegistry,
    args: &Arguments,
) -> Result<UpdatePendingWorkItemOk, PendingWorkError> {
    let id = args.identifier.required("update")?;
    let (title, title_normalized) = args
        .title
        .as_deref()
        .map(task_title)
        .transpose()?
        .map_or((None, false), |(title, normalized)| {
            (Some(title), normalized)
        });
    let updated = update_pending_work_item::execute(
        UpdatePendingWorkItem {
            id,
            prompt: args.prompt.clone(),
            title,
            append: args.append.clone(),
            prereq: args.prereq.clone(),
            clear_prereq: args.clear_prereq,
            commits: args.commits.clone(),
            append_report: args.append_report.clone(),
            effort: args.effort.map(Into::into),
            tags: args.tag.clone(),
            tags_clear: args.tags_clear,
        },
        store,
        projects,
    )
    .map_err(|error| PendingWorkError::ApplicationWrite(error.to_string()))?;
    if title_normalized {
        eprintln!("{TITLE_NORMALIZED_NOTICE}");
    }
    Ok(updated)
}
