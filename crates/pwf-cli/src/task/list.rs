use std::num::NonZeroUsize;

use clap::Args;
use pwf_application::{
    project::resolve_project::ResolveProjectError,
    task::list_tasks::{self, ListTasks, ListTasksError},
};
use pwf_infra::obsidian::ObsidianStore;
use pwf_models::{
    project::ProjectSelector,
    task::{TagInput, TaskTags},
};
use pwf_wire::task::{ListDetail, ListMode, ListScope, OrderDirection, OrderField, OrderSpec};

use super::{EffortChoice, SectionChoice, StatusChoice, TaskError, render::render_list};
use crate::console::Console;

#[derive(Args, Debug)]
pub struct Arguments {
    /// Limit to one project.
    #[arg(long)]
    pub(crate) project: Option<ProjectSelector>,
    /// Long form with per-task metadata.
    #[arg(long)]
    pub(crate) long: bool,
    /// Show only this scoped section.
    #[arg(long, value_enum, conflicts_with = "all")]
    pub(crate) section: Option<SectionChoice>,
    /// List everything: every section, every lifecycle status, no task cap.
    /// An explicit `--status` or `-n` overrides the widened default.
    #[arg(long)]
    pub(crate) all: bool,
    /// Cap to N listed tasks, N >= 1 [default: 10, or unlimited under `--all`].
    #[arg(short = 'n', long, value_name = "N", value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..=100_000))]
    pub(crate) number: Option<usize>,
    /// Show only tasks tagged with this exact effort/complexity tier.
    #[arg(long, value_enum)]
    pub(crate) effort: Option<EffortChoice>,
    /// Discovery tag filter; repeat or comma-separate for several. Every requested tag must
    /// match.
    #[arg(long, allow_hyphen_values = true)]
    pub(crate) tag: Vec<TagInput>,
    /// Sort key `field[:direction]`: field created|id|project-id, direction
    /// asc|desc [default: created:desc, flat across every listed project].
    /// `project-id` defaults to asc and groups by project, newest id first
    /// within each project.
    #[arg(short = 'o', long, value_name = "FIELD[:DIR]", value_parser = parse_order)]
    pub(crate) order: Option<OrderSpec>,
    /// Filter by one lifecycle status, or include every lifecycle status
    /// [default: active, or all under `--all`].
    #[arg(long, value_enum)]
    pub(crate) status: Option<StatusChoice>,
    #[arg(skip)]
    pub(crate) mode: ListMode,
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
) -> Result<String, TaskError> {
    let result = list_tasks::execute(
        &ListTasks {
            project_selector: arguments.project.clone(),
            scope: list_scope(arguments),
            number: arguments.number.and_then(NonZeroUsize::new),
            effort: arguments.effort.map(Into::into),
            tags: TaskTags::from_inputs(&arguments.tag),
            order: arguments.order,
            status: arguments.status.map(StatusChoice::filter),
            detail: if arguments.long {
                ListDetail::Detailed
            } else {
                ListDetail::Summary
            },
            mode: arguments.mode,
        },
        store,
        pool,
        store,
    )
    .await
    .map_err(map_list_tasks_error)?;
    let location = result.project_task_path.as_ref().map_or_else(
        || "managed project task paths".to_string(),
        ToString::to_string,
    );
    Ok(render_list(&result, &location, console.color()))
}

fn list_scope(arguments: &Arguments) -> ListScope {
    if arguments.all {
        return ListScope::All;
    }
    match arguments.section {
        Some(SectionChoice::Future) => ListScope::Future,
        Some(SectionChoice::Human) => ListScope::Human,
        None => ListScope::Default,
    }
}

/// Parses a `field[:direction]` sort key, using field-specific direction defaults.
fn parse_order(value: &str) -> Result<OrderSpec, String> {
    const USAGE: &str =
        "use field[:direction] with field created|id|project-id and direction asc|desc";
    let (field_token, direction_token) = match value.split_once(':') {
        Some((field, direction)) => (field, Some(direction)),
        None => (value, None),
    };
    let field = match field_token {
        "created" => OrderField::Created,
        "id" => OrderField::Id,
        "project-id" => OrderField::ProjectId,
        _ => return Err(USAGE.to_string()),
    };
    let direction = match direction_token {
        None => match field {
            OrderField::Created | OrderField::Id => OrderDirection::Desc,
            OrderField::ProjectId => OrderDirection::Asc,
        },
        Some("asc") => OrderDirection::Asc,
        Some("desc") => OrderDirection::Desc,
        Some(_) => return Err(USAGE.to_string()),
    };
    Ok(OrderSpec { field, direction })
}

fn map_list_tasks_error(error: ListTasksError) -> TaskError {
    match error {
        ListTasksError::ResolveProject(ResolveProjectError::Unknown { selector, known }) => {
            TaskError::UnknownManagedProject { selector, known }
        }
        error => TaskError::List(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_value_parses_field_and_optional_direction() {
        assert_eq!(
            parse_order("id").unwrap(),
            OrderSpec {
                field: OrderField::Id,
                direction: OrderDirection::Desc,
            }
        );
        assert_eq!(
            parse_order("created:asc").unwrap(),
            OrderSpec {
                field: OrderField::Created,
                direction: OrderDirection::Asc,
            }
        );
        assert_eq!(
            parse_order("project-id").unwrap(),
            OrderSpec {
                field: OrderField::ProjectId,
                direction: OrderDirection::Asc,
            }
        );
    }

    #[test]
    fn order_value_rejects_unknown_field_or_direction() {
        for bad in ["bogus", "asc", "created:sideways", "created:asc:desc"] {
            assert!(parse_order(bad).is_err(), "{bad}");
        }
    }
}
