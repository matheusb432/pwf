use clap::Args;
use pwf_application::{
    ports::clock::Clock,
    task::add_task::{self, AddTask, AddTaskPrompt},
};
use pwf_infra::obsidian::ObsidianStore;
use pwf_models::{
    project::ProjectSelector,
    task::{PrerequisiteInput, Prerequisites},
};

use super::{
    render::{
        TITLE_NORMALIZED_NOTICE, emit_created_section, emit_created_section_for_error, render_added,
    },
    shared::{EffortChoice, LaneFlagMode, TaskError, task_lanes, task_title},
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
    /// Prereq task id; repeat or comma-separate for several
    #[arg(long)]
    pub(crate) prereq: Vec<PrerequisiteInput>,
    /// Discovery tag; repeat or comma-separate for several. Input accepts `snake_case` or
    /// kebab-case
    #[arg(long, allow_hyphen_values = true)]
    pub(crate) tag: Vec<String>,
    /// Effort/complexity tier.
    #[arg(long, value_enum)]
    pub(crate) effort: Option<EffortChoice>,
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<String, TaskError> {
    let (prompt, title_normalized) = match arguments.title.as_deref() {
        Some(title) => {
            let (title, normalized) = task_title(title)?;
            let lanes = task_lanes(
                &arguments.goal,
                &arguments.context,
                &arguments.constraint,
                &arguments.done_when,
                LaneFlagMode::Add,
            )?;
            (AddTaskPrompt::Structured { title, lanes }, normalized)
        }
        None => (AddTaskPrompt::Shorthand(arguments.prompt.join(" ")), false),
    };
    let result = add_task::execute(
        &AddTask {
            project_selector: arguments.project.clone(),
            prompt,
            human: arguments.human,
            prerequisites: Prerequisites::from_inputs(&arguments.prereq),
            effort: arguments.effort.map(Into::into),
            tags: arguments.tag.clone(),
        },
        store,
        pool,
        clock,
    )
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
            Err(TaskError::Add(error))
        }
    }
}
