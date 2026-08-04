use clap::Args;
use pwf_application::task::update_task::{self, UpdateTask};
use pwf_infra::obsidian::ObsidianStore;
use pwf_models::task::{PrerequisiteInput, Prerequisites};

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
    /// Prereq task id to append (repeat or comma-separate); dedups.
    #[arg(long)]
    pub(crate) prereq: Vec<PrerequisiteInput>,
    /// Clear all prereqs on the task.
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
    /// works on closed done/cancelled tasks too.
    #[arg(long)]
    pub(crate) commits: Vec<String>,
    /// Append multi-line Markdown under `### Report` verbatim; works on closed
    /// done/cancelled tasks too.
    #[arg(long)]
    pub(crate) append_report: Option<String>,
    /// Splice lane-syntax bullets (same syntax as `add`'s prompt) into the body's
    /// Goals/Context/Constraints/Done When sections; active tasks only.
    #[arg(short = 'a', long, conflicts_with = "prompt")]
    pub(crate) append: Option<String>,
    /// Effort tier; active tasks only.
    #[arg(long, value_enum)]
    pub(crate) effort: Option<EffortChoice>,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

use super::{
    render::{TITLE_NORMALIZED_NOTICE, render_updated},
    shared::TaskError,
};
use crate::console::Console;

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
) -> Result<String, TaskError> {
    let id = arguments.identifier.required("update")?;
    let (title, title_normalized) = arguments
        .title
        .as_deref()
        .map(task_title)
        .transpose()?
        .map_or((None, false), |(title, normalized)| {
            (Some(title), normalized)
        });
    let updated = update_task::execute(
        UpdateTask {
            id,
            prompt: arguments.prompt.clone(),
            title,
            append: arguments.append.clone(),
            prereq: Prerequisites::from_inputs(&arguments.prereq),
            clear_prereq: arguments.clear_prereq,
            commits: arguments.commits.clone(),
            append_report: arguments.append_report.clone(),
            effort: arguments.effort.map(Into::into),
            tags: arguments.tag.clone(),
            tags_clear: arguments.tags_clear,
        },
        store,
        pool,
    )
    .await
    .map_err(|error| TaskError::ApplicationWrite(error.to_string()))?;
    if title_normalized {
        eprintln!("{TITLE_NORMALIZED_NOTICE}");
    }
    Ok(render_updated(&updated, console.color()))
}
