use clap::{ArgGroup, Args};
use pwf_application::task::{
    TaskLane, TaskLaneEdits,
    edit_task::{self, EditTask, EditTaskContent},
};
use pwf_infra::obsidian::ObsidianStore;
use pwf_models::task::{PrerequisiteInput, Prerequisites};

use super::{
    render::{TITLE_NORMALIZED_NOTICE, render_edited},
    shared::{EffortChoice, Identifier, LaneFlagMode, TaskError, task_lanes, task_title},
};
use crate::console::Console;

#[derive(Args, Debug)]
#[command(group(
    ArgGroup::new("edit")
        .required(true)
        .multiple(true)
        .args([
            "prompt",
            "title",
            "append",
            "add_goal",
            "remove_goals",
            "add_context",
            "remove_contexts",
            "add_constraint",
            "remove_constraints",
            "add_done_when",
            "remove_done_whens",
            "add_prereq",
            "remove_prereqs",
            "add_tag",
            "remove_tags",
            "effort",
            "remove_effort",
        ])
))]
#[expect(
    clippy::struct_excessive_bools,
    reason = "clap mirrors independent remove actions"
)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Replace the title and every prompt lane using shorthand lane syntax.
    #[arg(
        long,
        conflicts_with_all = [
            "title",
            "append",
            "add_goal",
            "remove_goals",
            "add_context",
            "remove_contexts",
            "add_constraint",
            "remove_constraints",
            "add_done_when",
            "remove_done_whens"
        ]
    )]
    pub(crate) prompt: Option<String>,
    /// Replace the title. The normalized title cannot exceed 200 characters.
    #[arg(long, conflicts_with = "prompt")]
    pub(crate) title: Option<String>,
    /// Append shorthand lane content without changing the title.
    #[arg(
        short = 'a',
        long,
        conflicts_with_all = [
            "prompt",
            "add_goal",
            "remove_goals",
            "add_context",
            "remove_contexts",
            "add_constraint",
            "remove_constraints",
            "add_done_when",
            "remove_done_whens"
        ]
    )]
    pub(crate) append: Option<String>,
    /// Append a Goal bullet; repeat for several.
    #[arg(long, conflicts_with_all = ["prompt", "append"])]
    pub(crate) add_goal: Vec<String>,
    /// Remove every Goal before applying `--add-goal` values.
    #[arg(long, conflicts_with_all = ["prompt", "append"])]
    pub(crate) remove_goals: bool,
    /// Append a Context bullet; repeat for several.
    #[arg(long, conflicts_with_all = ["prompt", "append"])]
    pub(crate) add_context: Vec<String>,
    /// Remove every Context before applying `--add-context` values.
    #[arg(long, conflicts_with_all = ["prompt", "append"])]
    pub(crate) remove_contexts: bool,
    /// Append a Constraint bullet; repeat for several.
    #[arg(long, conflicts_with_all = ["prompt", "append"])]
    pub(crate) add_constraint: Vec<String>,
    /// Remove every Constraint before applying `--add-constraint` values.
    #[arg(long, conflicts_with_all = ["prompt", "append"])]
    pub(crate) remove_constraints: bool,
    /// Append a Done When bullet; repeat for several.
    #[arg(long, conflicts_with_all = ["prompt", "append"])]
    pub(crate) add_done_when: Vec<String>,
    /// Remove every Done When before applying `--add-done-when` values.
    #[arg(long, conflicts_with_all = ["prompt", "append"])]
    pub(crate) remove_done_whens: bool,
    /// Append a prerequisite task ID; repeat or comma-separate for several.
    #[arg(long)]
    pub(crate) add_prereq: Vec<PrerequisiteInput>,
    /// Remove every prerequisite before applying `--add-prereq` values.
    #[arg(long)]
    pub(crate) remove_prereqs: bool,
    /// Append a discovery tag; repeat or comma-separate for several.
    #[arg(long, allow_hyphen_values = true)]
    pub(crate) add_tag: Vec<String>,
    /// Remove every tag before applying `--add-tag` values.
    #[arg(long)]
    pub(crate) remove_tags: bool,
    /// Replace the effort tier.
    #[arg(long, value_enum, conflicts_with = "remove_effort")]
    pub(crate) effort: Option<EffortChoice>,
    /// Remove the effort tier.
    #[arg(long, conflicts_with = "effort")]
    pub(crate) remove_effort: bool,
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
) -> Result<String, TaskError> {
    let id = arguments.identifier.required("edit")?;
    let (title, title_normalized) = arguments
        .title
        .as_deref()
        .map(task_title)
        .transpose()?
        .map_or((None, false), |(title, normalized)| {
            (Some(title), normalized)
        });
    let additions = task_lanes(
        &arguments.add_goal,
        &arguments.add_context,
        &arguments.add_constraint,
        &arguments.add_done_when,
        LaneFlagMode::Edit,
    )?;
    let removals = [
        arguments.remove_goals.then_some(TaskLane::Goal),
        arguments.remove_contexts.then_some(TaskLane::Context),
        arguments.remove_constraints.then_some(TaskLane::Constraint),
        arguments.remove_done_whens.then_some(TaskLane::DoneWhen),
    ]
    .into_iter()
    .flatten();
    let lanes = TaskLaneEdits::new(additions, removals);
    let content = if let Some(prompt) = arguments.prompt.as_ref() {
        Some(EditTaskContent::ReplaceShorthand {
            prompt: prompt.clone(),
        })
    } else if let Some(prompt) = arguments.append.as_ref() {
        Some(EditTaskContent::AppendShorthand {
            title,
            prompt: prompt.clone(),
        })
    } else if title.is_some() || !lanes.is_empty() {
        Some(EditTaskContent::Structured { title, lanes })
    } else {
        None
    };
    let edited = edit_task::execute(
        EditTask {
            id,
            content,
            add_prerequisites: Prerequisites::from_inputs(&arguments.add_prereq),
            remove_prerequisites: arguments.remove_prereqs,
            effort: arguments.effort.map(Into::into),
            remove_effort: arguments.remove_effort,
            add_tags: arguments.add_tag.clone(),
            remove_tags: arguments.remove_tags,
        },
        store,
        pool,
    )
    .await
    .map_err(|error| TaskError::ApplicationWrite(error.to_string()))?;
    if title_normalized {
        eprintln!("{TITLE_NORMALIZED_NOTICE}");
    }
    Ok(render_edited(&edited, console.color()))
}
