use clap::{Args, FromArgMatches, Subcommand};
use pwf_client::{
    pb::TaskStatusFilter,
    project::ProjectClient,
    task::{TaskClient, TaskDag},
};
use pwf_models::{
    session::Agent,
    settings::UserSettings,
    task::{EffortTier, TaskId, TaskTitle},
};

use crate::console::Console;

mod activate;
mod add;
mod add_from_file;
mod backlog;
mod blocked_by_input;
mod cancel;
mod clone;
mod dag;
mod done;
mod edit;
mod get;
mod list;
mod remove;
mod render;
pub mod route;
mod sections;
pub mod session;

/// Renders the default task-DAG view for the Criterion benchmark.
#[doc(hidden)]
#[must_use]
pub fn benchmark_dag_render(task_dag: TaskDag, color_on: bool) -> String {
    dag::render(task_dag, color_on)
}

/// Prepares the default task-DAG view for allocation measurement.
#[doc(hidden)]
pub fn benchmark_dag_render_prepare(task_dag: TaskDag, color_on: bool) {
    dag::benchmark_prepare(task_dag, color_on);
}

#[derive(Args, Debug)]
#[group(required = true)]
struct TaskIdentifierArguments {
    /// Task id (bare positional; `--id` also accepted). E.g. `PWF-0001`, `cfg57`.
    #[arg(value_name = "ID")]
    task_id_positional: Option<String>,
    #[arg(value_name = "NUMBER", hide = true, requires = "task_id_positional")]
    task_id_number: Option<String>,
    #[arg(long = "id", value_name = "ID", conflicts_with = "task_id_positional")]
    task_id_flag: Option<TaskId>,
}

#[derive(Debug)]
pub(crate) struct Identifier {
    task_id: TaskId,
}

impl Identifier {
    fn id(&self) -> &TaskId {
        &self.task_id
    }
}

impl TryFrom<TaskIdentifierArguments> for Identifier {
    type Error = clap::Error;

    fn try_from(arguments: TaskIdentifierArguments) -> Result<Self, Self::Error> {
        let task_id_positional = match (arguments.task_id_positional, arguments.task_id_number) {
            (Some(task_id), Some(number)) => Some(format!("{task_id}-{number}")),
            (Some(task_id), None) => Some(task_id),
            (None, None) => None,
            (None, Some(_)) => {
                return Err(clap::Error::raw(
                    clap::error::ErrorKind::MissingRequiredArgument,
                    "a split task ID number requires its project code",
                ));
            }
        }
        .map(|task_id| {
            task_id.parse::<TaskId>().map_err(|error| {
                clap::Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!("invalid value '{task_id}' for '[ID]': {error}"),
                )
            })
        })
        .transpose()?;

        let task_id = match (task_id_positional, arguments.task_id_flag) {
            (Some(_), Some(_)) => {
                return Err(clap::Error::raw(
                    clap::error::ErrorKind::ArgumentConflict,
                    "a task ID cannot be supplied by both position and --id",
                ));
            }
            (Some(task_id), None) | (None, Some(task_id)) => task_id,
            (None, None) => {
                return Err(clap::Error::raw(
                    clap::error::ErrorKind::MissingRequiredArgument,
                    "a task ID is required",
                ));
            }
        };
        Ok(Self { task_id })
    }
}

impl FromArgMatches for Identifier {
    fn from_arg_matches(matches: &clap::ArgMatches) -> Result<Self, clap::Error> {
        TaskIdentifierArguments::from_arg_matches(matches)?.try_into()
    }

    fn update_from_arg_matches(&mut self, matches: &clap::ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

impl Args for Identifier {
    fn group_id() -> Option<clap::Id> {
        TaskIdentifierArguments::group_id()
    }

    fn augment_args(command: clap::Command) -> clap::Command {
        TaskIdentifierArguments::augment_args(command)
    }

    fn augment_args_for_update(command: clap::Command) -> clap::Command {
        TaskIdentifierArguments::augment_args_for_update(command)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum AgentChoice {
    Claude,
    // TODO: make default agent choice be configurable by user
    #[default]
    Codex,
}

impl From<AgentChoice> for Agent {
    fn from(choice: AgentChoice) -> Self {
        match choice {
            AgentChoice::Claude => Self::Claude,
            AgentChoice::Codex => Self::Codex,
        }
    }
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum StatusChoice {
    Active,
    Backlog,
    Done,
    Cancelled,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum EffortChoice {
    Low,
    Medium,
    High,
    Highest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum PriorityChoice {
    Low,
    Medium,
    High,
    Highest,
}

impl From<EffortChoice> for EffortTier {
    fn from(choice: EffortChoice) -> Self {
        match choice {
            EffortChoice::Low => Self::Low,
            EffortChoice::Medium => Self::Medium,
            EffortChoice::High => Self::High,
            EffortChoice::Highest => Self::Highest,
        }
    }
}

impl StatusChoice {
    fn filter(self) -> TaskStatusFilter {
        match self {
            Self::Active => TaskStatusFilter::Active,
            Self::Backlog => TaskStatusFilter::Backlog,
            Self::Done => TaskStatusFilter::Done,
            Self::Cancelled => TaskStatusFilter::Cancelled,
            Self::All => TaskStatusFilter::All,
        }
    }
}

fn task_title(raw: &str) -> anyhow::Result<(TaskTitle, bool)> {
    if raw.trim().is_empty() {
        return Err(anyhow::anyhow!("Task title cannot be empty."));
    }
    let title = TaskTitle::try_new(raw).map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let normalized = title.as_ref() != raw.trim();
    Ok((title, normalized))
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Manages pwf tasks
    Task(TaskArguments),
    /// Dispatch an agent session into a project's cwd
    Session(session::Arguments),
    /// Internal word router behind bare `pwf <words...>`
    #[command(hide = true)]
    Route(route::Arguments),
}

#[derive(Args, Debug)]
pub struct TaskArguments {
    #[command(subcommand)]
    command: TaskCommand,
}

#[derive(Subcommand, Debug)]
enum TaskCommand {
    /// Add a task from shorthand, Markdown, or a Markdown file
    Add(Box<add::Arguments>),
    /// Copy a task into a new active task, optionally in another project.
    Clone(clone::Arguments),
    /// List tasks. `--all` lists everything
    #[command(alias = "ls")]
    List(list::Arguments),
    /// Mark a task done in its file
    Done(done::Arguments),
    /// Mark a task cancelled in its file
    Cancel(cancel::Arguments),
    /// Defer an active task to backlog, hiding it from default lists
    Backlog(backlog::Arguments),
    /// Activate a task; closed tasks require confirmation to clear completion data
    Activate(activate::Arguments),
    /// Edit an active or backlogged task's content or metadata
    Edit(Box<edit::Arguments>),
    /// Show a task's full content
    #[command(alias = "g")]
    Get(get::Arguments),
    /// Show a task's directed dependency graph
    Dag(dag::Arguments),
    /// Delete a task file
    Remove(remove::Arguments),
    /// Show the shorthand section markers and how each section renders
    Sections(sections::Arguments),
}

pub async fn run(
    command: &Command,
    console: Console,
    settings: &UserSettings,
    client: &TaskClient,
    projects: &ProjectClient,
) -> Result<String, crate::error::Error> {
    let task_status_colors = settings.task_status_colors();
    let output = match command {
        Command::Task(arguments) => match &arguments.command {
            TaskCommand::Add(arguments) => {
                add::run(arguments, console, settings, client, projects).await?
            }
            TaskCommand::Clone(arguments) => {
                clone::run(arguments, console, settings, client, projects).await?
            }
            TaskCommand::List(arguments) => {
                list::run(arguments, console, settings, client, projects).await?
            }
            TaskCommand::Done(arguments) => done::run(arguments, console, settings, client).await?,
            TaskCommand::Cancel(arguments) => {
                cancel::run(arguments, console, settings, client).await?
            }
            TaskCommand::Activate(arguments) => {
                activate::run(arguments, console, settings, client).await?
            }
            TaskCommand::Backlog(arguments) => {
                backlog::run(arguments, console, settings, client).await?
            }
            TaskCommand::Edit(arguments) => edit::run(arguments, console, settings, client).await?,
            TaskCommand::Get(arguments) => {
                get::run(arguments, console, settings, client, projects).await?
            }
            TaskCommand::Dag(arguments) => {
                dag::run(arguments, console, task_status_colors, client).await?
            }
            TaskCommand::Remove(arguments) => {
                remove::run(arguments, console, settings, client).await?
            }
            TaskCommand::Sections(arguments) => sections::run(arguments, client).await?,
        },
        Command::Session(arguments) => session::run(arguments, console, client).await?,
        Command::Route(arguments) => match route::resolve(arguments) {
            route::ResolvedCommand::List(arguments) => {
                list::run(&arguments, console, settings, client, projects).await?
            }
            route::ResolvedCommand::RejectUnsupportedTaskCreation => {
                return Err(anyhow::anyhow!("Use: pwf task add <project> \"<body>\"").into());
            }
        },
    };
    Ok(output)
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
enum ContentFormat {
    Rich,
    Md,
    Json,
}

impl ContentFormat {
    fn for_console(console: Console) -> Self {
        if console.stdout_terminal() {
            Self::Rich
        } else {
            Self::Md
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ContentSelection {
    Automatic,
    Format(ContentFormat),
}

impl ContentSelection {
    fn parse(value: &str) -> Result<Self, String> {
        if value.is_empty() {
            return Ok(Self::Automatic);
        }
        <ContentFormat as clap::ValueEnum>::from_str(value, false).map(Self::Format)
    }

    fn resolve(self, console: Console) -> ContentFormat {
        match self {
            Self::Automatic => ContentFormat::for_console(console),
            Self::Format(format) => format,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::task_title;

    #[test]
    fn task_title_reports_whitespace_normalization_only() {
        for (raw, expected, normalized) in [
            ("  Fix The THING  ", "Fix The THING", false),
            ("time is 3:30pm", "time is 3:30pm", false),
            (
                "fix parser: handle colons",
                "fix parser: handle colons",
                false,
            ),
            ("Fix\n  #123; Parser", "Fix #123; Parser", true),
        ] {
            let (title, was_normalized) = task_title(raw).unwrap();
            assert_eq!(title.as_ref(), expected);
            assert_eq!(was_normalized, normalized);
        }
    }

    #[test]
    fn task_title_rejects_blank_machine_input() {
        assert!(task_title(" \t ").is_err());
    }
}
