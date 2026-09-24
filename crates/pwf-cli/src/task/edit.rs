use clap::{ArgGroup, Args};
use pwf_client::{
    pb::{
        self, AppendTaskBody, ClearField, StringCollectionEdit, TaskContentEdit, UpdateTaskRequest,
        effort_edit, priority_edit, task_content_edit,
    },
    task::TaskClient,
};
use pwf_models::{
    settings::TaskStatusColors,
    task::{TagInput, TaskTags, TaskTitle},
};

use super::{
    EffortChoice, Identifier, PriorityChoice,
    blocked_by_input::{self, BlockedByInput},
    render::{TITLE_NORMALIZED_NOTICE, TaskMutationAction, render_mutation},
    task_title,
};
use crate::{console::Console, edit::string_collection_edit};

const EDIT: &str = "task_edit";

#[derive(Args, Debug)]
#[command(group(ArgGroup::new(EDIT).required(true).multiple(true)))]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Replace the title and body with shorthand; leading text becomes the title. See `pwf task
    /// sections`.
    #[arg(long, group = EDIT, conflicts_with_all = ["title", "append_body"])]
    pub(crate) replace_body: Option<String>,
    /// Replace the title. The normalized title cannot exceed 200 characters.
    #[arg(long, group = EDIT)]
    pub(crate) title: Option<String>,
    /// Append shorthand items to their sections; leading text joins the first section.
    #[arg(short = 'a', long, group = EDIT)]
    pub(crate) append_body: Option<String>,
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
struct BlockedByEdits {
    /// Append a blocked-by task ID or [[ID]]; repeat or comma-separate for several.
    #[arg(short = 'b', long, group = EDIT)]
    add_blocked_by: Vec<BlockedByInput>,
    /// Remove every blocked-by task before applying `--add-blocked-by` values.
    #[arg(long, group = EDIT)]
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
    #[arg(short = 't', long, group = EDIT, allow_hyphen_values = true)]
    add_tag: Vec<TagInput>,
    /// Remove every tag before applying `--add-tag` values.
    #[arg(long, group = EDIT)]
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
#[group(multiple = false)]
struct EffortEdit {
    /// Replace the effort tier.
    #[arg(short = 'e', long, value_enum, group = EDIT)]
    effort: Option<EffortChoice>,
    /// Remove the effort tier.
    #[arg(long, group = EDIT)]
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
#[group(multiple = false)]
struct PriorityEdit {
    /// Replace the priority tier.
    #[arg(short = 'p', long, value_enum, group = EDIT)]
    priority: Option<PriorityChoice>,
    /// Remove the priority tier.
    #[arg(long, group = EDIT)]
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
    let id = arguments.identifier.id();
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
    };
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
    let content = if let Some(body) = arguments.replace_body.as_ref() {
        task_content_edit::Content::Replace(body.clone())
    } else if let Some(body) = arguments.append_body.as_ref() {
        if body.trim().is_empty() {
            return Err(anyhow::anyhow!("--append-body cannot be empty."));
        }
        task_content_edit::Content::Append(AppendTaskBody {
            title: title.map(ToString::to_string),
            body: body.clone(),
        })
    } else if let Some(title) = title {
        task_content_edit::Content::Title(title.to_string())
    } else {
        return Ok(None);
    };
    Ok(Some(TaskContentEdit {
        content: Some(content),
    }))
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
