use std::num::NonZeroUsize;

use clap::Args;
use pwf_client::{
    pb::{
        AllTaskSections, EffortTier, ListDetail, ListTasksRequest, OrderDirection, OrderField,
        OrderSpec, PriorityTier, list_tasks_request,
    },
    task::TaskClient,
};
use pwf_models::{
    project::ProjectSelector,
    settings::TaskStatusColors,
    task::{TagInput, TaskSection, TaskTags},
};

use super::{EffortChoice, PriorityChoice, StatusChoice, render::render_list};
use crate::console::Console;

const TASK_LIST_PAGE_SIZE: u32 = 256;

#[derive(Args, Debug)]
pub struct Arguments {
    /// Limit to one project.
    #[arg(long)]
    pub(crate) project: Option<ProjectSelector>,
    /// Long form with per-task metadata.
    #[arg(long)]
    pub(crate) long: bool,
    /// Show only the section whose `##` header matches this text, ignoring case.
    #[arg(long, value_name = "HEADER", conflicts_with = "all")]
    pub(crate) section: Option<TaskSection>,
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
    /// Show only tasks with this scheduling priority.
    #[arg(long, value_enum)]
    pub(crate) priority: Option<PriorityChoice>,
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
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    task_status_colors: TaskStatusColors,
    client: &TaskClient,
) -> anyhow::Result<String> {
    let mut request = ListTasksRequest {
        project_selector: arguments.project.as_ref().map(ToString::to_string),
        scope: list_scope(arguments),
        number: arguments
            .number
            .and_then(NonZeroUsize::new)
            .map(|value| value.get() as u64),
        effort: arguments.effort.map(|value| match value {
            EffortChoice::Low => EffortTier::Low as i32,
            EffortChoice::Medium => EffortTier::Medium as i32,
            EffortChoice::High => EffortTier::High as i32,
            EffortChoice::Highest => EffortTier::Highest as i32,
        }),
        tags: TaskTags::from_inputs(&arguments.tag)
            .map(|tags| tags.iter().map(ToString::to_string).collect())
            .unwrap_or_default(),
        order: arguments.order,
        status: arguments.status.map(|status| status.filter() as i32),
        detail: if arguments.long {
            ListDetail::Detailed as i32
        } else {
            ListDetail::Summary as i32
        },
        priority: arguments.priority.map(|value| match value {
            PriorityChoice::Low => PriorityTier::Low as i32,
            PriorityChoice::Medium => PriorityTier::Medium as i32,
            PriorityChoice::High => PriorityTier::High as i32,
            PriorityChoice::Highest => PriorityTier::Highest as i32,
        }),
        page_size: TASK_LIST_PAGE_SIZE,
        page_token: None,
    };
    let mut result = client
        .list_tasks(request.clone())
        .await
        .map_err(crate::rpc_error)?;
    let mut seen_tokens = std::collections::HashSet::new();
    while let Some(token) = result.next_page_token.take() {
        if !seen_tokens.insert(token.clone()) {
            return Err(anyhow::anyhow!(
                "pwf-server repeated a task-list continuation token"
            ));
        }
        request.page_token = Some(token);
        let mut page = client
            .list_tasks(request.clone())
            .await
            .map_err(crate::rpc_error)?;
        result.tasks.append(&mut page.tasks);
        result.next_page_token = page.next_page_token;
    }
    let location = result.project_task_path.as_ref().map_or_else(
        || "managed project task paths".to_string(),
        ToString::to_string,
    );
    Ok(render_list(
        &result,
        &location,
        task_status_colors,
        console.color(),
    ))
}

fn list_scope(arguments: &Arguments) -> Option<list_tasks_request::Scope> {
    if arguments.all {
        return Some(list_tasks_request::Scope::All(AllTaskSections {}));
    }
    arguments
        .section
        .as_ref()
        .map(|section| list_tasks_request::Scope::Section(section.to_string()))
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
            OrderField::Unspecified => return Err(USAGE.to_string()),
        },
        Some("asc") => OrderDirection::Asc,
        Some("desc") => OrderDirection::Desc,
        Some(_) => return Err(USAGE.to_string()),
    };
    Ok(OrderSpec {
        field: field as i32,
        direction: direction as i32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_value_parses_field_and_optional_direction() {
        assert_eq!(
            parse_order("id").unwrap(),
            OrderSpec {
                field: OrderField::Id as i32,
                direction: OrderDirection::Desc as i32,
            }
        );
        assert_eq!(
            parse_order("created:asc").unwrap(),
            OrderSpec {
                field: OrderField::Created as i32,
                direction: OrderDirection::Asc as i32,
            }
        );
        assert_eq!(
            parse_order("project-id").unwrap(),
            OrderSpec {
                field: OrderField::ProjectId as i32,
                direction: OrderDirection::Asc as i32,
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
