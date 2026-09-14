use clap::Args;
use pwf_client::{
    pb::{CreateTaskRequest, EffortTier, PriorityTier, StructuredTaskBody, create_task_request},
    task::TaskClient,
};
use pwf_models::{
    project::ProjectId,
    settings::TaskStatusColors,
    task::{TagInput, TaskTags},
};

use super::{
    EffortChoice, MarkerSectionFlagMode, PriorityChoice,
    blocked_by_input::{self, BlockedByInput},
    render::{TITLE_NORMALIZED_NOTICE, TaskMutationAction, render_mutation},
    task_marker_sections, task_title,
};
use crate::console::Console;

#[derive(Args, Debug)]
pub struct Arguments {
    /// Managed project ID (two to four ASCII letters)
    #[arg(value_name = "PROJECT", value_parser = crate::project::parse_project_id)]
    pub(crate) project: ProjectId,
    /// Task body shorthand. Conflicts with marker-section-specific args.
    #[arg(value_name = "BODY")]
    pub(crate) body: Vec<String>,
    #[command(flatten)]
    structured: StructuredBody,
    /// Blocked-by task ID or [[ID]]; repeat or comma-separate for several
    #[arg(short = 'b', long)]
    pub(crate) blocked_by: Vec<BlockedByInput>,
    /// Discovery tag; repeat or comma-separate for several. Input accepts `snake_case` or
    /// kebab-case
    #[arg(short = 't', long, allow_hyphen_values = true)]
    pub(crate) tag: Vec<TagInput>,
    /// Effort/complexity tier.
    #[arg(short = 'e', long, value_enum)]
    pub(crate) effort: Option<EffortChoice>,
    /// Scheduling priority tier.
    #[arg(short = 'p', long, value_enum)]
    pub(crate) priority: Option<PriorityChoice>,
}

#[derive(Args, Debug)]
#[group(id = "structured_body", conflicts_with = "body", requires = "title")]
struct StructuredBody {
    /// Task's title
    #[arg(long, required_unless_present = "body")]
    pub(crate) title: Option<String>,
    /// Goal. repeat for several. Requires `--title`
    #[arg(long)]
    pub(crate) goal: Vec<String>,
    /// Context. repeat for several. Requires `--title`
    #[arg(long)]
    pub(crate) context: Vec<String>,
    /// Constraint. repeat for several. Requires `--title`
    #[arg(long)]
    pub(crate) constraint: Vec<String>,
    /// Done When. repeat for several. Requires `--title`
    #[arg(long)]
    pub(crate) done_when: Vec<String>,
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    task_status_colors: TaskStatusColors,
    client: &TaskClient,
    projects: &pwf_client::project::ProjectClient,
) -> Result<String, crate::error::Error> {
    let (body, title_normalized) = request_body(arguments)?;
    let project_id = crate::project::resolve_project_id(&arguments.project, projects).await?;
    let result = client
        .create_task(CreateTaskRequest {
            project_id: project_id.to_string(),
            body: Some(body),
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
            if title_normalized {
                eprintln!("{TITLE_NORMALIZED_NOTICE}");
            }
            render_mutation(
                TaskMutationAction::Added,
                &added.id,
                added.task.as_ref(),
                task_status_colors,
                console.color(),
            )
            .map_err(Into::into)
        }
        Err(error) => Err(error.into()),
    }
}

fn request_body(arguments: &Arguments) -> anyhow::Result<(create_task_request::Body, bool)> {
    if let Some(title) = arguments.structured.title.as_deref() {
        let (title, normalized) = task_title(title)?;
        let sections = task_marker_sections(
            &arguments.structured.goal,
            &arguments.structured.context,
            &arguments.structured.constraint,
            &arguments.structured.done_when,
            MarkerSectionFlagMode::Add,
        )?;
        return Ok((
            create_task_request::Body::Structured(StructuredTaskBody {
                title: title.to_string(),
                sections: Some(sections),
            }),
            normalized,
        ));
    }

    let body = arguments.body.join(" ");
    if body.trim().is_empty() {
        return Err(anyhow::anyhow!(
            "Use shorthand: pwf task add <project> \"<body>\"\nOr machine mode: pwf task add <project> --title <title> [marker-section flags]"
        ));
    }
    Ok((create_task_request::Body::Shorthand(body), false))
}
