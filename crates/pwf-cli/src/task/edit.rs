use clap::{ArgGroup, Args};
use pwf_application::task::{
    TaskLane, TaskLaneEdits,
    edit_task::{self, CollectionEdit, EditTask, EditTaskContent, TaskEdits, ValueEdit},
};
use pwf_infra::obsidian::ObsidianStore;
use pwf_models::task::{EffortTier, PrerequisiteInput, Prerequisites, TagInput, Tags};

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
    #[command(flatten)]
    goals: GoalEdits,
    #[command(flatten)]
    contexts: ContextEdits,
    #[command(flatten)]
    constraints: ConstraintEdits,
    #[command(flatten)]
    done_whens: DoneWhenEdits,
    #[command(flatten)]
    prerequisites: PrerequisiteEdits,
    #[command(flatten)]
    tags: TagEdits,
    #[command(flatten)]
    effort: EffortEdit,
}

#[derive(Args, Debug)]
struct GoalEdits {
    /// Append a Goal bullet; repeat for several.
    #[arg(long, conflicts_with_all = ["prompt", "append"])]
    add_goal: Vec<String>,
    /// Remove every Goal before applying `--add-goal` values.
    #[arg(long, conflicts_with_all = ["prompt", "append"])]
    remove_goals: bool,
}

#[derive(Args, Debug)]
struct ContextEdits {
    /// Append a Context bullet; repeat for several.
    #[arg(long, conflicts_with_all = ["prompt", "append"])]
    add_context: Vec<String>,
    /// Remove every Context before applying `--add-context` values.
    #[arg(long, conflicts_with_all = ["prompt", "append"])]
    remove_contexts: bool,
}

#[derive(Args, Debug)]
struct ConstraintEdits {
    /// Append a Constraint bullet; repeat for several.
    #[arg(long, conflicts_with_all = ["prompt", "append"])]
    add_constraint: Vec<String>,
    /// Remove every Constraint before applying `--add-constraint` values.
    #[arg(long, conflicts_with_all = ["prompt", "append"])]
    remove_constraints: bool,
}

#[derive(Args, Debug)]
struct DoneWhenEdits {
    /// Append a Done When bullet; repeat for several.
    #[arg(long, conflicts_with_all = ["prompt", "append"])]
    add_done_when: Vec<String>,
    /// Remove every Done When before applying `--add-done-when` values.
    #[arg(long, conflicts_with_all = ["prompt", "append"])]
    remove_done_whens: bool,
}

#[derive(Args, Debug)]
struct PrerequisiteEdits {
    /// Append a prerequisite task ID; repeat or comma-separate for several.
    #[arg(long)]
    add_prereq: Vec<PrerequisiteInput>,
    /// Remove every prerequisite before applying `--add-prereq` values.
    #[arg(long)]
    remove_prereqs: bool,
}

impl PrerequisiteEdits {
    fn edit(&self) -> CollectionEdit<Prerequisites> {
        collection_edit(
            Prerequisites::from_inputs(&self.add_prereq),
            self.remove_prereqs,
        )
    }
}

#[derive(Args, Debug)]
struct TagEdits {
    /// Append a discovery tag; repeat or comma-separate for several.
    #[arg(long, allow_hyphen_values = true)]
    add_tag: Vec<TagInput>,
    /// Remove every tag before applying `--add-tag` values.
    #[arg(long)]
    remove_tags: bool,
}

impl TagEdits {
    fn edit(&self) -> CollectionEdit<Tags> {
        collection_edit(Tags::from_inputs(&self.add_tag), self.remove_tags)
    }
}

#[derive(Args, Debug)]
struct EffortEdit {
    /// Replace the effort tier.
    #[arg(long, value_enum, conflicts_with = "remove_effort")]
    effort: Option<EffortChoice>,
    /// Remove the effort tier.
    #[arg(long, conflicts_with = "effort")]
    remove_effort: bool,
}

impl EffortEdit {
    fn edit(&self) -> ValueEdit<EffortTier> {
        match self.effort {
            Some(effort) => ValueEdit::Set(effort.into()),
            None if self.remove_effort => ValueEdit::Clear,
            None => ValueEdit::Unchanged,
        }
    }
}

fn collection_edit<T>(addition: Option<T>, remove_existing: bool) -> CollectionEdit<T> {
    match (addition, remove_existing) {
        (Some(values), true) => CollectionEdit::Replace(values),
        (Some(values), false) => CollectionEdit::Append(values),
        (None, true) => CollectionEdit::Clear,
        (None, false) => CollectionEdit::Unchanged,
    }
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
        &arguments.goals.add_goal,
        &arguments.contexts.add_context,
        &arguments.constraints.add_constraint,
        &arguments.done_whens.add_done_when,
        LaneFlagMode::Edit,
    )?;
    let removals = [
        arguments.goals.remove_goals.then_some(TaskLane::Goal),
        arguments
            .contexts
            .remove_contexts
            .then_some(TaskLane::Context),
        arguments
            .constraints
            .remove_constraints
            .then_some(TaskLane::Constraint),
        arguments
            .done_whens
            .remove_done_whens
            .then_some(TaskLane::DoneWhen),
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
    let edits = TaskEdits::try_new(
        content,
        arguments.prerequisites.edit(),
        arguments.effort.edit(),
        arguments.tags.edit(),
    )
    .map_err(|error| TaskError::ApplicationWrite(error.to_string()))?;
    let edited = edit_task::execute(EditTask { id, edits }, store, pool)
        .await
        .map_err(|error| TaskError::ApplicationWrite(error.to_string()))?;
    if title_normalized {
        eprintln!("{TITLE_NORMALIZED_NOTICE}");
    }
    Ok(render_edited(&edited, console.color()))
}
