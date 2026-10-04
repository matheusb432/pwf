use anyhow::Context as _;
use clap::{Args, Subcommand};
use pwf_client::{
    pb::{CreateTaskBody, CreateTaskRequest, EffortTier, PriorityTier, create_task_request},
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
    render::{TITLE_NORMALIZED_NOTICE, TaskMutationAction, render_mutation},
    task_title,
};
use crate::console::Console;

#[derive(Args, Debug)]
#[command(
    args_conflicts_with_subcommands = true,
    after_long_help = "Examples:\n  pwf task add PWF \"Fix task rendering / preserve headings /d tests pass\"\n  pwf task add PWF \"Fix task rendering\" --body \"$(cat draft.md)\"\n  pwf task add PWF \"Empty draft\" --body ''"
)]
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
    /// Title when --body is supplied; otherwise shorthand: title, then items after `/` and section
    /// markers. See `pwf task sections`.
    #[arg(value_name = "TITLE_OR_SHORTHAND")]
    input: Vec<String>,
    /// Verbatim Markdown body; positional text supplies the title. An empty value creates an empty
    /// body.
    #[arg(
        long,
        value_name = "MARKDOWN",
        requires = "input",
        allow_hyphen_values = true
    )]
    body: Option<String>,
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
    let (content, title_normalized) = request_content(arguments)?;
    let project = arguments
        .project
        .as_ref()
        .context("direct task creation requires a project")?;
    let project_id = crate::project::resolve_project_id(project, projects).await?;
    let result = client
        .create_task(CreateTaskRequest {
            project_id: project_id.to_string(),
            content: Some(content),
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
                settings,
                console.color(),
            )
            .map_err(Into::into)
        }
        Err(error) => Err(error.into()),
    }
}

fn request_content(
    arguments: &DirectArguments,
) -> anyhow::Result<(create_task_request::Content, bool)> {
    let input = arguments.input.join(" ");
    if let Some(body) = arguments.body.as_ref() {
        let (title, normalized) = task_title(&input)?;
        return Ok((
            create_task_request::Content::Body(CreateTaskBody {
                title: title.to_string(),
                body: body.clone(),
            }),
            normalized,
        ));
    }
    if input.trim().is_empty() {
        return Err(anyhow::anyhow!(
            "Use shorthand: pwf task add <project> \"<shorthand>\"\nOr Markdown: pwf task add <project> \"<title>\" --body \"<markdown>\"\nRun `pwf task sections <project>` to see the section markers."
        ));
    }
    Ok((create_task_request::Content::Shorthand(input), false))
}
