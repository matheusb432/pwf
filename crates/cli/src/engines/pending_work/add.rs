use clap::Args;
use pwf_application::{
    Clock,
    pending_work::{
        PendingWorkSection, ProjectRegistry,
        add::{self, AddPendingWorkError, AddPendingWorkItem, AddPendingWorkSource},
    },
};
use pwf_infra::obsidian::ObsidianStore;

use super::{
    common::{CommonArguments, PendingWorkError},
    render::{
        ADD_MIRROR_REMEDY, TITLE_NORMALIZED_NOTICE, emit_add_diagnostics, emit_created_section,
        emit_created_section_for_error, render_added,
    },
};
use crate::console::Console;

#[derive(Args, Debug)]
pub struct Arguments {
    /// Managed project (full name or id code, case-insensitive).
    #[arg(value_name = "PROJECT")]
    pub(crate) project: Option<String>,
    /// Task prompt words (joined with single spaces).
    #[arg(value_name = "PROMPT")]
    pub(crate) prompt: Vec<String>,
    /// Build the prompt from the repo's newest handoff.
    #[arg(long = "continue-handoff", conflicts_with_all = ["prompt", "continue_path"])]
    pub(crate) continue_handoff: bool,
    /// Build the prompt to continue the plan at PATH.
    #[arg(long = "continue", value_name = "PATH", conflicts_with_all = ["prompt", "continue_handoff"])]
    pub(crate) continue_path: Option<String>,
    /// File the item under a section (future|human|low-prio).
    #[arg(long, value_name = "SECTION")]
    pub(crate) section: Option<String>,
    /// Explicit title (else inferred from the prompt). YAML-breaking
    /// characters (e.g. a colon before a space) are normalized with a
    /// stderr notice so the note's frontmatter stays parseable.
    #[arg(long)]
    pub(crate) title: Option<String>,
    /// File the item under `## Human` (shorthand for `--section human`).
    #[arg(long)]
    pub(crate) human: bool,
    /// Prereq item id; repeat or comma-separate for several.
    #[arg(long)]
    pub(crate) prereq: Vec<String>,
    /// Discovery tag; repeat or comma-separate for several. Input accepts `snake_case` or
    /// kebab-case.
    #[arg(long, allow_hyphen_values = true)]
    pub(crate) tag: Vec<String>,
    /// Effort/complexity tier (1=easy .. 4=xhard); optional. Picks a Claude model
    /// via config/model-tiers.toml when the item is later dispatched with `pwf
    /// session` (codex ignores it).
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=4))]
    pub(crate) effort: Option<u8>,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

pub(super) fn run(
    arguments: &Arguments,
    console: Console,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
    clock: &impl Clock,
) -> Result<String, PendingWorkError> {
    let command = request(arguments)?;
    let result = add::execute(&command, store, projects, clock);
    match result {
        Ok(added) => {
            emit_created_section(&added);
            if added.title_normalized {
                eprintln!("{TITLE_NORMALIZED_NOTICE}");
            }
            if let pwf_application::handoff::HandoffMutationOk::Created { path } = &added.handoff {
                eprintln!("info: created handoff {}", path.display());
            }
            Ok(render_added(&added, console.color()))
        }
        Err(error) => {
            emit_created_section_for_error(&error);
            match error {
                AddPendingWorkError::HandoffPreflight(source) => {
                    Err(PendingWorkError::HandoffLifecycle(source))
                }
                AddPendingWorkError::HandoffAfterPendingWork {
                    pending_work_identifier,
                    diagnostics,
                    source,
                } => {
                    emit_add_diagnostics(&diagnostics);
                    Err(PendingWorkError::HandoffLifecycleAfterMutation {
                        id: pending_work_identifier.to_string(),
                        source,
                        remedy: ADD_MIRROR_REMEDY.to_string(),
                    })
                }
                other => Err(PendingWorkError::Add(other)),
            }
        }
    }
}

fn request(arguments: &Arguments) -> Result<AddPendingWorkItem, PendingWorkError> {
    let section = match arguments.section.as_deref() {
        Some(section) => PendingWorkSection::from_name(section)
            .map(Some)
            .ok_or_else(|| PendingWorkError::BadSection {
                value: section.to_string(),
            })?,
        None => arguments.human.then_some(PendingWorkSection::Human),
    };

    let source = if arguments.continue_handoff {
        Some(AddPendingWorkSource::NewestHandoff)
    } else if let Some(path) = arguments.continue_path.clone() {
        Some(AddPendingWorkSource::Plan { path })
    } else if arguments.prompt.is_empty() {
        None
    } else {
        Some(AddPendingWorkSource::Prompt {
            prompt: arguments.prompt.join(" "),
            title: arguments.title.clone(),
        })
    };
    Ok(AddPendingWorkItem {
        project_identifier: arguments.project.clone(),
        source,
        date: arguments.common.date.clone(),
        section,
        prerequisites: arguments.prereq.clone(),
        effort: arguments.effort,
        tags: arguments.tag.clone(),
    })
}
