use clap::{Args, FromArgMatches, Subcommand};
use pwf_client::{
    pb::{TaskLane, TaskLanes, TaskStatusFilter},
    task::{TaskClient, TaskDag},
};
use pwf_models::{
    session::Agent,
    settings::TaskStatusColors,
    task::{EffortTier, TaskId, TaskTitle},
};

use crate::console::Console;

mod add;
mod blocked_by_input;
mod cancel;
mod dag;
mod done;
mod edit;
mod get;
mod list;
mod remove;
mod render;
mod reopen;
pub mod route;
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
    task_id: Option<TaskId>,
}

impl Identifier {
    fn required<Error>(&self, error: Error) -> Result<TaskId, Error> {
        self.task_id.clone().ok_or(error)
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
            (Some(task_id), None) | (None, Some(task_id)) => Some(task_id),
            (None, None) => None,
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
            Self::Done => TaskStatusFilter::Done,
            Self::Cancelled => TaskStatusFilter::Cancelled,
            Self::All => TaskStatusFilter::All,
        }
    }
}

fn task_title(raw: &str) -> anyhow::Result<(TaskTitle, bool)> {
    if raw.trim().is_empty() {
        return Err(anyhow::anyhow!("--title cannot be empty."));
    }
    let comparison = raw.trim().to_lowercase();
    let title = TaskTitle::try_new(raw).map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let normalized = title.as_ref() != comparison;
    Ok((title, normalized))
}

#[derive(Debug, Clone, Copy)]
enum LaneFlagMode {
    Add,
    Edit,
}

impl LaneFlagMode {
    fn flag(self, lane: TaskLane) -> &'static str {
        match (self, lane) {
            (Self::Add, TaskLane::Goal) => "--goal",
            (Self::Add, TaskLane::Context) => "--context",
            (Self::Add, TaskLane::Constraint) => "--constraint",
            (Self::Add, TaskLane::DoneWhen) => "--done-when",
            (Self::Edit, TaskLane::Goal) => "--add-goal",
            (Self::Edit, TaskLane::Context) => "--add-context",
            (Self::Edit, TaskLane::Constraint) => "--add-constraint",
            (Self::Edit, TaskLane::DoneWhen) => "--add-done-when",
            (_, TaskLane::Unspecified) => "--lane",
        }
    }
}

fn task_lanes(
    goals: &[String],
    context: &[String],
    constraints: &[String],
    done_when: &[String],
    mode: LaneFlagMode,
) -> anyhow::Result<TaskLanes> {
    Ok(TaskLanes {
        goals: normalize_lanes(goals, mode.flag(TaskLane::Goal))?,
        context: normalize_lanes(context, mode.flag(TaskLane::Context))?,
        constraints: normalize_lanes(constraints, mode.flag(TaskLane::Constraint))?,
        done_when: normalize_lanes(done_when, mode.flag(TaskLane::DoneWhen))?,
    })
}

fn normalize_lanes(values: &[String], flag: &str) -> anyhow::Result<Vec<String>> {
    values
        .iter()
        .map(|value| {
            if value.contains(['\n', '\r']) {
                return Err(anyhow::anyhow!("{flag} must be a single line."));
            }
            let value = value.trim();
            if value.is_empty() {
                return Err(anyhow::anyhow!("{flag} cannot be empty."));
            }
            Ok(value.to_string())
        })
        .collect()
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
    /// Add a pwf task with shorthand prompt text or explicit args
    Add(add::Arguments),
    /// List tasks. `--all` lists everything
    #[command(alias = "ls")]
    List(list::Arguments),
    /// Mark a task done in place, keeping a capped done-queue
    Done(done::Arguments),
    /// Mark a task cancelled in place, keeping the same capped queue as done
    Cancel(cancel::Arguments),
    /// Reopen a closed task after confirming deletion of its completion data
    Reopen(reopen::Arguments),
    /// Edit an active task's prompt body, title, blocked-by tasks, tags, or effort
    Edit(Box<edit::Arguments>),
    /// Get a task note's markdown
    #[command(alias = "g")]
    Get(get::Arguments),
    /// Show a task's directed dependency graph
    Dag(dag::Arguments),
    /// Delete a task note and remove its index link
    Remove(remove::Arguments),
}

pub async fn run(
    command: &Command,
    console: Console,
    task_status_colors: TaskStatusColors,
    client: &TaskClient,
) -> anyhow::Result<String> {
    let output = match command {
        Command::Task(arguments) => match &arguments.command {
            TaskCommand::Add(arguments) => add::run(arguments, console, client).await?,
            TaskCommand::List(arguments) => {
                list::run(arguments, console, task_status_colors, client).await?
            }
            TaskCommand::Done(arguments) => done::run(arguments, client).await?,
            TaskCommand::Cancel(arguments) => cancel::run(arguments, client).await?,
            TaskCommand::Reopen(arguments) => reopen::run(arguments, console, client).await?,
            TaskCommand::Edit(arguments) => edit::run(arguments, console, client).await?,
            TaskCommand::Get(arguments) => get::run(arguments, client).await?,
            TaskCommand::Dag(arguments) => {
                dag::run(arguments, console, task_status_colors, client).await?
            }
            TaskCommand::Remove(arguments) => remove::run(arguments, console, client).await?,
        },
        Command::Session(arguments) => session::run(arguments, console, client).await?,
        Command::Route(arguments) => match route::resolve(arguments) {
            route::ResolvedCommand::List(arguments) => {
                list::run(&arguments, console, task_status_colors, client).await?
            }
            route::ResolvedCommand::RejectUnsupportedTaskCreation => {
                return Err(anyhow::anyhow!("Use: pwf task add <project> \"<prompt>\""));
            }
        },
    };
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{LaneFlagMode, task_lanes, task_title};

    #[test]
    fn task_title_reports_metadata_normalization_only() {
        for (raw, expected, normalized) in [
            ("  Fix The THING  ", "fix the thing", false),
            ("time is 3:30pm", "time is 3:30pm", false),
            (
                "fix parser: handle colons",
                "fix parser; handle colons",
                true,
            ),
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

    #[test]
    fn task_lanes_report_the_owning_machine_flag() {
        let empty_goal =
            task_lanes(&["  ".to_string()], &[], &[], &[], LaneFlagMode::Add).unwrap_err();
        assert_eq!(empty_goal.to_string(), "--goal cannot be empty.");

        let multiline_context = task_lanes(
            &[],
            &["first\nsecond".to_string()],
            &[],
            &[],
            LaneFlagMode::Edit,
        )
        .unwrap_err();
        assert_eq!(
            multiline_context.to_string(),
            "--add-context must be a single line."
        );
    }
}
