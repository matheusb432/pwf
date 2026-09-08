use clap::{ArgGroup, Args};
use pwf_client::{
    pb::{
        self, AppendTaskPrompt, ClearField, StringCollectionEdit, StructuredTaskEdit,
        TaskContentEdit, TaskLane, UpdateTaskRequest, effort_edit, priority_edit,
        task_content_edit,
    },
    task::TaskClient,
};
use pwf_models::{
    settings::TaskStatusColors,
    task::{TagInput, TaskTags, TaskTitle},
};

use super::{
    EffortChoice, Identifier, LaneFlagMode, PriorityChoice,
    blocked_by_input::{self, BlockedByInput},
    render::{TITLE_NORMALIZED_NOTICE, TaskMutationAction, render_mutation},
    task_lanes, task_title,
};
use crate::{console::Console, edit::string_collection_edit};

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
            "add_blocked_by",
            "remove_blocked_by",
            "add_tag",
            "remove_tags",
            "effort",
            "remove_effort",
            "priority",
            "remove_priority",
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
    blocked_by: BlockedByEdits,
    #[command(flatten)]
    tags: TagEdits,
    #[command(flatten)]
    effort: EffortEdit,
    #[command(flatten)]
    priority: PriorityEdit,
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
struct BlockedByEdits {
    /// Append a blocked-by task ID or [[ID]]; repeat or comma-separate for several.
    #[arg(long)]
    add_blocked_by: Vec<BlockedByInput>,
    /// Remove every blocked-by task before applying `--add-blocked-by` values.
    #[arg(long)]
    remove_blocked_by: bool,
}

impl BlockedByEdits {
    fn edit(&self) -> Option<StringCollectionEdit> {
        string_collection_edit(
            blocked_by_input::collect(&self.add_blocked_by)
                .map(|values| values.iter().map(ToString::to_string).collect())
                .unwrap_or_default(),
            self.remove_blocked_by,
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
    fn edit(&self) -> Option<StringCollectionEdit> {
        string_collection_edit(
            TaskTags::from_inputs(&self.add_tag)
                .map(|tags| tags.iter().map(ToString::to_string).collect())
                .unwrap_or_default(),
            self.remove_tags,
        )
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
    fn edit(&self) -> Option<pb::EffortEdit> {
        self.effort.map_or_else(
            || {
                self.remove_effort.then_some(pb::EffortEdit {
                    operation: Some(effort_edit::Operation::Clear(ClearField {})),
                })
            },
            |effort| {
                Some(pb::EffortEdit {
                    operation: Some(effort_edit::Operation::Set(wire_effort(effort) as i32)),
                })
            },
        )
    }
}

#[derive(Args, Debug)]
struct PriorityEdit {
    /// Replace the priority tier.
    #[arg(long, value_enum, conflicts_with = "remove_priority")]
    priority: Option<PriorityChoice>,
    /// Remove the priority tier.
    #[arg(long, conflicts_with = "priority")]
    remove_priority: bool,
}

impl PriorityEdit {
    fn edit(&self) -> Option<pb::PriorityEdit> {
        self.priority.map_or_else(
            || {
                self.remove_priority.then_some(pb::PriorityEdit {
                    operation: Some(priority_edit::Operation::Clear(ClearField {})),
                })
            },
            |priority| {
                Some(pb::PriorityEdit {
                    operation: Some(priority_edit::Operation::Set(wire_priority(priority) as i32)),
                })
            },
        )
    }
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    task_status_colors: TaskStatusColors,
    client: &TaskClient,
) -> anyhow::Result<String> {
    let id = arguments
        .identifier
        .required(anyhow::anyhow!("--id is required for edit."))?;
    let (title, title_normalized) = arguments
        .title
        .as_deref()
        .map(task_title)
        .transpose()?
        .map_or((None, false), |(title, normalized)| {
            (Some(title), normalized)
        });
    let content = content_edit(arguments, title.as_ref())?;
    let request = UpdateTaskRequest {
        id: id.to_string(),
        content,
        blocked_by: arguments.blocked_by.edit(),
        effort: arguments.effort.edit(),
        tags: arguments.tags.edit(),
        priority: arguments.priority.edit(),
        expected_revision: None,
        request_id: String::new(),
    };
    if request.content.is_none()
        && request.blocked_by.is_none()
        && request.effort.is_none()
        && request.tags.is_none()
        && request.priority.is_none()
    {
        return Err(anyhow::anyhow!(
            "nothing to edit; pass at least one edit flag."
        ));
    }
    let result = client
        .update_task(request)
        .await
        .map_err(crate::rpc_error)?;
    if title_normalized {
        eprintln!("{TITLE_NORMALIZED_NOTICE}");
    }
    render_mutation(
        TaskMutationAction::Edited,
        id.as_ref(),
        result.task.as_ref(),
        task_status_colors,
        console.color(),
    )
}

fn content_edit(
    arguments: &Arguments,
    title: Option<&TaskTitle>,
) -> anyhow::Result<Option<TaskContentEdit>> {
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
    let removals = removals.map(|lane| lane as i32).collect::<Vec<_>>();
    let content = if let Some(prompt) = arguments.prompt.as_ref() {
        Some(TaskContentEdit {
            content: Some(task_content_edit::Content::Replace(prompt.clone())),
        })
    } else if let Some(prompt) = arguments.append.as_ref() {
        if prompt.trim().is_empty() {
            return Err(anyhow::anyhow!("--append cannot be empty."));
        }
        Some(TaskContentEdit {
            content: Some(task_content_edit::Content::Append(AppendTaskPrompt {
                title: title.map(ToString::to_string),
                prompt: prompt.clone(),
            })),
        })
    } else if title.is_some()
        || !additions.goals.is_empty()
        || !additions.context.is_empty()
        || !additions.constraints.is_empty()
        || !additions.done_when.is_empty()
        || !removals.is_empty()
    {
        Some(TaskContentEdit {
            content: Some(task_content_edit::Content::Structured(StructuredTaskEdit {
                title: title.map(ToString::to_string),
                additions: Some(additions),
                removals,
            })),
        })
    } else {
        None
    };
    Ok(content)
}

fn wire_effort(value: EffortChoice) -> pb::EffortTier {
    match value {
        EffortChoice::Low => pb::EffortTier::Low,
        EffortChoice::Medium => pb::EffortTier::Medium,
        EffortChoice::High => pb::EffortTier::High,
        EffortChoice::Highest => pb::EffortTier::Highest,
    }
}

fn wire_priority(value: PriorityChoice) -> pb::PriorityTier {
    match value {
        PriorityChoice::Low => pb::PriorityTier::Low,
        PriorityChoice::Medium => pb::PriorityTier::Medium,
        PriorityChoice::High => pb::PriorityTier::High,
        PriorityChoice::Highest => pb::PriorityTier::Highest,
    }
}
