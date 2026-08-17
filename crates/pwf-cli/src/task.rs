use clap::{Args, Subcommand};
use pwf_application::{
    ports::clock::Clock,
    task::{CloseTaskError, resolve_task_project::ResolveTaskProjectError},
};
use pwf_infra::obsidian::ObsidianStore;
use pwf_models::{
    project::HomeDirectory,
    session::Agent,
    task::{EffortTier, TaskId, TaskStatus, TaskTitle},
};
use pwf_wire::task::{
    AddTaskApiError, CloseTaskApiError, ResolveTaskProjectApiError, StatusFilter,
    TaskInputApiError, TaskLane, TaskLanes,
};

use crate::console::Console;

mod add;
mod cancel;
mod done;
mod edit;
mod get;
mod list;
mod remove;
mod render;
mod reopen;
pub mod route;
pub mod session;

#[derive(Args, Debug, Default)]
pub(crate) struct Identifier {
    /// Task id (bare positional; `--id` also accepted). E.g. `PWF-0001`, `cfg57`.
    #[arg(value_name = "ID")]
    positional: Option<TaskId>,
    #[arg(long = "id", value_name = "ID", conflicts_with = "positional")]
    flag: Option<TaskId>,
}

impl Identifier {
    fn required<Error>(&self, error: Error) -> Result<TaskId, Error> {
        self.positional
            .as_ref()
            .or(self.flag.as_ref())
            .cloned()
            .ok_or(error)
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
pub(crate) enum SectionChoice {
    Future,
    Human,
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
    fn filter(self) -> StatusFilter {
        match self {
            Self::Active => StatusFilter::Exact(TaskStatus::Active),
            Self::Done => StatusFilter::Exact(TaskStatus::Done),
            Self::Cancelled => StatusFilter::Exact(TaskStatus::Cancelled),
            Self::All => StatusFilter::All,
        }
    }
}

fn task_title(raw: &str) -> Result<(TaskTitle, bool), TaskInputApiError> {
    if raw.trim().is_empty() {
        return Err(TaskInputApiError::EmptyTitle);
    }
    let comparison = raw.trim().to_lowercase();
    let title = TaskTitle::try_new(raw).map_err(|error| TaskInputApiError::InvalidTitle {
        message: error.to_string(),
    })?;
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
        }
    }
}

fn task_lanes(
    goals: &[String],
    context: &[String],
    constraints: &[String],
    done_when: &[String],
    mode: LaneFlagMode,
) -> Result<TaskLanes, TaskInputApiError> {
    TaskLanes::try_new(
        goals.to_vec(),
        context.to_vec(),
        constraints.to_vec(),
        done_when.to_vec(),
    )
    .map_err(|error| TaskInputApiError::InvalidLaneValue {
        flag: mode.flag(error.lane()),
        reason: error.reason(),
    })
}

fn map_resolve_task_project_error(error: ResolveTaskProjectError) -> ResolveTaskProjectApiError {
    match error {
        ResolveTaskProjectError::UnknownProjectId {
            task_id,
            project_id,
        } => ResolveTaskProjectApiError::UnknownProjectId {
            task_id,
            project_id,
        },
        ResolveTaskProjectError::QueryProject(source) => ResolveTaskProjectApiError::Unexpected {
            message: source.to_string(),
        },
    }
}

fn map_close_task_error(error: CloseTaskError) -> CloseTaskApiError {
    match error {
        CloseTaskError::TaskNotFound { id } => CloseTaskApiError::TaskNotFound { id },
        CloseTaskError::UnknownProjectId {
            task_id,
            project_id,
        } => CloseTaskApiError::ResolveProject(ResolveTaskProjectApiError::UnknownProjectId {
            task_id,
            project_id,
        }),
        CloseTaskError::InvalidTitle { id, source } => CloseTaskApiError::InvalidTitle {
            id,
            reason: source.to_string(),
        },
        CloseTaskError::WriteStore(source) => CloseTaskApiError::Unexpected {
            message: source.to_string(),
        },
        CloseTaskError::ReviewTask(source) => {
            CloseTaskApiError::ReviewTask(Box::new(add::map_add_task_error(source)))
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Manages pwf tasks
    Task(TaskArguments),
    /// Dispatch an agent session into a project's cwd, via tmux session or inline
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
    /// Delete a task note and remove its index link
    Remove(remove::Arguments),
}

pub async fn run(
    command: &Command,
    console: Console,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
    home: &HomeDirectory,
    clock: &impl Clock,
) -> anyhow::Result<String> {
    let output = match command {
        Command::Task(arguments) => match &arguments.command {
            TaskCommand::Add(arguments) => add::run(arguments, console, store, pool, clock).await?,
            TaskCommand::List(arguments) => list::run(arguments, console, store, pool).await?,
            TaskCommand::Done(arguments) => done::run(arguments, store, pool, clock).await?,
            TaskCommand::Cancel(arguments) => cancel::run(arguments, store, pool, clock).await?,
            TaskCommand::Reopen(arguments) => reopen::run(arguments, console, store, pool).await?,
            TaskCommand::Edit(arguments) => edit::run(arguments, console, store, pool).await?,
            TaskCommand::Get(arguments) => get::run(arguments, store, pool).await?,
            TaskCommand::Remove(arguments) => remove::run(arguments, console, store, pool).await?,
        },
        Command::Session(arguments) => session::run(arguments, console, store, pool, home).await?,
        Command::Route(arguments) => match route::resolve(arguments) {
            route::ResolvedCommand::List(arguments) => {
                list::run(&arguments, console, store, pool).await?
            }
            route::ResolvedCommand::RejectUnsupportedTaskCreation => {
                return Err(AddTaskApiError::UnsupportedTaskCreation.into());
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
