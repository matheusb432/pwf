use clap::Args;
use pwf_client::{
    task::TaskClient,
    v1::{
        AddTaskRequest, EffortTier, IndexSection, PriorityTier, StructuredTaskPrompt,
        add_task_request,
    },
};
use pwf_models::{
    project::ProjectSelector,
    task::{TagInput, TaskTags},
};

use super::{
    EffortChoice, LaneFlagMode, PriorityChoice,
    blocked_by_input::{self, BlockedByInput},
    render::{
        TITLE_NORMALIZED_NOTICE, emit_created_section, emit_created_section_for_error, render_added,
    },
    task_lanes, task_title,
};
use crate::console::Console;

#[derive(Args, Debug)]
pub struct Arguments {
    /// Project's name or id
    #[arg(value_name = "PROJECT")]
    pub(crate) project: Option<ProjectSelector>,
    /// Task's prompt's shorthand, using lanes. Conflicts with section-specific args.
    #[arg(
        value_name = "PROMPT",
        conflicts_with_all = ["title", "goal", "context", "constraint", "done_when"]
    )]
    pub(crate) prompt: Vec<String>,
    /// Task's title
    #[arg(long, required_unless_present = "prompt", conflicts_with = "prompt")]
    pub(crate) title: Option<String>,
    /// Goal. repeat for several. Requires `--title`
    #[arg(long, requires = "title", conflicts_with = "prompt")]
    pub(crate) goal: Vec<String>,
    /// Context. repeat for several. Requires `--title`
    #[arg(long, requires = "title", conflicts_with = "prompt")]
    pub(crate) context: Vec<String>,
    /// Constraint. repeat for several. Requires `--title`
    #[arg(long, requires = "title", conflicts_with = "prompt")]
    pub(crate) constraint: Vec<String>,
    /// Done When. repeat for several. Requires `--title`
    #[arg(long, requires = "title", conflicts_with = "prompt")]
    pub(crate) done_when: Vec<String>,
    /// File the task under `## Human` index section
    #[arg(long)]
    pub(crate) human: bool,
    /// Blocked-by task ID or [[ID]]; repeat or comma-separate for several
    #[arg(long)]
    pub(crate) blocked_by: Vec<BlockedByInput>,
    /// Discovery tag; repeat or comma-separate for several. Input accepts `snake_case` or
    /// kebab-case
    #[arg(long, allow_hyphen_values = true)]
    pub(crate) tag: Vec<TagInput>,
    /// Effort/complexity tier.
    #[arg(long, value_enum)]
    pub(crate) effort: Option<EffortChoice>,
    /// Scheduling priority tier.
    #[arg(long, value_enum)]
    pub(crate) priority: Option<PriorityChoice>,
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    client: &TaskClient,
) -> anyhow::Result<String> {
    let project_selector = arguments
        .project
        .clone()
        .ok_or_else(|| anyhow::anyhow!(
            "Use shorthand: pwf task add <project> \"<prompt>\"\nOr machine mode: pwf task add <project> --title <title> [lane flags]"
        ))?;
    let (prompt, title_normalized) = request_prompt(arguments)?;
    let result = client
        .add_task(AddTaskRequest {
            project_selector: project_selector.to_string(),
            prompt: Some(prompt),
            index_section: if arguments.human {
                IndexSection::Human as i32
            } else {
                IndexSection::General as i32
            },
            blocked_by: blocked_by_input::collect(&arguments.blocked_by)
                .map(|values| values.iter().map(ToString::to_string).collect())
                .unwrap_or_default(),
            effort: arguments.effort.map(|effort| match effort {
                super::EffortChoice::Low => EffortTier::Low as i32,
                super::EffortChoice::Medium => EffortTier::Medium as i32,
                super::EffortChoice::High => EffortTier::High as i32,
                super::EffortChoice::Highest => EffortTier::Highest as i32,
            }),
            tags: TaskTags::from_inputs(&arguments.tag)
                .map(|tags| tags.iter().map(ToString::to_string).collect())
                .unwrap_or_default(),
            priority: arguments.priority.map(|priority| match priority {
                PriorityChoice::Low => PriorityTier::Low as i32,
                PriorityChoice::Medium => PriorityTier::Medium as i32,
                PriorityChoice::High => PriorityTier::High as i32,
                PriorityChoice::Highest => PriorityTier::Highest as i32,
            }),
        })
        .await;
    match result {
        Ok(added) => {
            emit_created_section(&added);
            if title_normalized {
                eprintln!("{TITLE_NORMALIZED_NOTICE}");
            }
            Ok(render_added(&added, console.color()))
        }
        Err(error) => {
            emit_created_section_for_error(&error);
            Err(crate::rpc_error(error))
        }
    }
}

fn request_prompt(arguments: &Arguments) -> anyhow::Result<(add_task_request::Prompt, bool)> {
    if let Some(title) = arguments.title.as_deref() {
        let (title, normalized) = task_title(title)?;
        let lanes = task_lanes(
            &arguments.goal,
            &arguments.context,
            &arguments.constraint,
            &arguments.done_when,
            LaneFlagMode::Add,
        )?;
        return Ok((
            add_task_request::Prompt::Structured(StructuredTaskPrompt {
                title: title.to_string(),
                lanes: Some(lanes),
            }),
            normalized,
        ));
    }

    let prompt = arguments.prompt.join(" ");
    if prompt.trim().is_empty() {
        return Err(anyhow::anyhow!(
            "Use shorthand: pwf task add <project> \"<prompt>\"\nOr machine mode: pwf task add <project> --title <title> [lane flags]"
        ));
    }
    Ok((add_task_request::Prompt::Shorthand(prompt), false))
}
