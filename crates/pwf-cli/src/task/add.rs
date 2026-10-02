use anyhow::Context as _;
use clap::{Args, Subcommand};
use pwf_client::{
    pb::{CreateTaskRequest, EffortTier, PriorityTier},
    task::TaskClient,
};
use pwf_models::{
    project::ProjectId,
    settings::UserSettings,
    task::{TagInput, TaskTags},
};

use super::{
    EffortChoice, PriorityChoice,
    blocked_by_input::{self, BlockedByInput},
    render::{TaskMutationAction, render_mutation},
};
use crate::console::Console;

#[derive(Args, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Arguments {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    direct: DirectArguments,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Create a task whose title and body come from a Markdown file
    FromFile(super::add_from_file::Arguments),
}

#[derive(Args, Debug)]
struct DirectArguments {
    /// Managed project ID (two to four ASCII letters)
    #[arg(
        value_name = "PROJECT",
        value_parser = crate::project::parse_project_id,
        required = true
    )]
    project: Option<ProjectId>,
    /// Task body shorthand: a title, then items after `/` and section markers. See `pwf task
    /// sections`.
    #[arg(value_name = "BODY")]
    body: Vec<String>,
    /// Blocked-by task ID or [[ID]]; repeat or comma-separate for several
    #[arg(short = 'b', long)]
    blocked_by: Vec<BlockedByInput>,
    /// Discovery tag; repeat or comma-separate for several. Input accepts `snake_case` or
    /// kebab-case
    #[arg(short = 't', long, allow_hyphen_values = true)]
    tag: Vec<TagInput>,
    /// Effort/complexity tier.
    #[arg(short = 'e', long, value_enum)]
    effort: Option<EffortChoice>,
    /// Scheduling priority tier.
    #[arg(short = 'p', long, value_enum)]
    priority: Option<PriorityChoice>,
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    settings: &UserSettings,
    client: &TaskClient,
    projects: &pwf_client::project::ProjectClient,
) -> Result<String, crate::error::Error> {
    match arguments.command.as_ref() {
        Some(Command::FromFile(arguments)) => {
            super::add_from_file::run(arguments, console, settings, client, projects).await
        }
        None => run_direct(&arguments.direct, console, settings, client, projects).await,
    }
}

async fn run_direct(
    arguments: &DirectArguments,
    console: Console,
    settings: &UserSettings,
    client: &TaskClient,
    projects: &pwf_client::project::ProjectClient,
) -> Result<String, crate::error::Error> {
    let shorthand = request_shorthand(arguments)?;
    let project = arguments
        .project
        .as_ref()
        .context("direct task creation requires a project")?;
    let project_id = crate::project::resolve_project_id(project, projects).await?;
    let result = client
        .create_task(CreateTaskRequest {
            project_id: project_id.to_string(),
            shorthand,
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
        Ok(added) => render_mutation(
            TaskMutationAction::Added,
            &added.id,
            added.task.as_ref(),
            settings,
            console.color(),
        )
        .map_err(Into::into),
        Err(error) => Err(error.into()),
    }
}

fn request_shorthand(arguments: &DirectArguments) -> anyhow::Result<String> {
    let body = arguments.body.join(" ");
    if body.trim().is_empty() {
        return Err(anyhow::anyhow!(
            "Use shorthand: pwf task add <project> \"<body>\"\nRun `pwf task sections <project>` to see the section markers."
        ));
    }
    Ok(body)
}
