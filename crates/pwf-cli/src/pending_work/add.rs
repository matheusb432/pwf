use clap::Args;
use pwf_application::{
    pending_work::{
        ProjectRegistry,
        add_pending_work_item::{self, AddPendingWorkItem},
    },
    ports::clock::Clock,
};
use pwf_infra::obsidian::ObsidianStore;

use super::{
    render::{
        TITLE_NORMALIZED_NOTICE, emit_created_section, emit_created_section_for_error, render_added,
    },
    shared::{CommonArguments, EffortChoice, PendingWorkError, task_title},
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
    /// Build the prompt to continue the plan at PATH.
    #[arg(long = "continue", value_name = "PATH", conflicts_with = "prompt")]
    pub(crate) continue_path: Option<String>,
    /// File the item under a section (future|human|low-prio).
    #[arg(long, value_name = "SECTION")]
    pub(crate) section: Option<String>,
    /// Explicit title (else inferred from the prompt). YAML-breaking
    /// characters (e.g. a colon before a space) are normalized with a
    /// stderr notice so the note's frontmatter stays parseable. The normalized
    /// title cannot exceed 200 characters.
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
    /// Effort/complexity tier; optional. Picks a Claude model via config/model-tiers.toml when
    /// the item is later dispatched with `pwf session` (codex ignores it).
    #[arg(long, value_enum)]
    pub(crate) effort: Option<EffortChoice>,
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
    let (title, title_normalized) = arguments
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .map(task_title)
        .transpose()?
        .map_or((None, false), |(title, normalized)| {
            (Some(title), normalized)
        });
    let result = add_pending_work_item::execute(
        &AddPendingWorkItem {
            project_identifier: arguments.project.clone(),
            prompt: arguments.prompt.join(" "),
            continue_path: arguments.continue_path.clone(),
            title,
            date: arguments.common.date.clone(),
            section: arguments.section.clone(),
            human: arguments.human,
            prerequisites: arguments.prereq.clone(),
            effort: arguments.effort.map(Into::into),
            tags: arguments.tag.clone(),
        },
        store,
        projects,
        clock,
    );
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
            Err(PendingWorkError::Add(error))
        }
    }
}
