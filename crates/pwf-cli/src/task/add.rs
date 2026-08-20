use clap::Args;
use pwf_application::{
    ports::clock::Clock,
    task::add_task::{self, AddTaskError},
};
use pwf_infra::obsidian::ObsidianStore;
use pwf_models::{
    project::ProjectSelector,
    task::{IndexSection, TagInput, TaskPrompt, TaskTags},
};
use pwf_wire::task::{AddTask, AddTaskApiError, AddTaskPrompt, TaskInputApiError};

use super::{
    EffortChoice, LaneFlagMode,
    blocked_by_input::{self, BlockedByInput},
    render::{
        TITLE_NORMALIZED_NOTICE, emit_created_section, emit_created_section_for_error, render_added,
    },
    task_lanes, task_title,
};
use crate::{console::Console, project::map_resolve_project_error};

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
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<String, AddTaskApiError> {
    let project_selector = arguments
        .project
        .clone()
        .ok_or(AddTaskApiError::InvalidRequest)?;
    let (prompt, title_normalized) = request_prompt(arguments)?;
    let result = add_task::execute(
        &AddTask {
            project_selector,
            prompt,
            index_section: if arguments.human {
                IndexSection::Human
            } else {
                IndexSection::default()
            },
            blocked_by: blocked_by_input::collect(&arguments.blocked_by),
            effort: arguments.effort.map(Into::into),
            tags: TaskTags::from_inputs(&arguments.tag),
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
            let error = map_add_task_error(error);
            emit_created_section_for_error(&error);
            Err(error)
        }
    }
}

fn request_prompt(arguments: &Arguments) -> Result<(AddTaskPrompt, bool), AddTaskApiError> {
    if let Some(title) = arguments.title.as_deref() {
        let (title, normalized) = task_title(title)?;
        let lanes = task_lanes(
            &arguments.goal,
            &arguments.context,
            &arguments.constraint,
            &arguments.done_when,
            LaneFlagMode::Add,
        )?;
        return Ok((AddTaskPrompt::structured(title, lanes), normalized));
    }

    let prompt = AddTaskPrompt::shorthand(TaskPrompt::new(arguments.prompt.join(" ")))
        .map_err(|_| AddTaskApiError::InvalidRequest)?;
    Ok((prompt, false))
}

pub(super) fn map_add_task_error(error: AddTaskError) -> AddTaskApiError {
    match error {
        AddTaskError::ProjectResolution(error) => map_resolve_project_error(error).into(),
        AddTaskError::QueryProject(source) | AddTaskError::AllocateTaskId { source, .. } => {
            AddTaskApiError::Unexpected {
                message: source.to_string(),
            }
        }
        AddTaskError::UnknownBlockedByIds { ids } => AddTaskApiError::UnknownBlockedByIds { ids },
        AddTaskError::ReadBlockedBy { id, source } => AddTaskApiError::ReadBlockedBy {
            id,
            reason: source.to_string(),
        },
        AddTaskError::SelfBlockedBy { target, blocker } => {
            AddTaskApiError::SelfBlockedBy { target, blocker }
        }
        AddTaskError::BlockedByCycle { path } => AddTaskApiError::BlockedByCycle { path },
        AddTaskError::MalformedBlockedBy {
            task,
            path,
            raw,
            reason,
        } => AddTaskApiError::MalformedBlockedBy {
            task,
            path,
            raw,
            reason,
        },
        AddTaskError::InvalidTitle(source) => TaskInputApiError::InvalidTitle {
            message: source.to_string(),
        }
        .into(),
        AddTaskError::WriteStore {
            diagnostics,
            source,
        } => AddTaskApiError::WriteStore {
            diagnostics,
            message: source.to_string(),
        },
    }
}
