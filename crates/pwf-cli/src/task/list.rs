use std::num::NonZeroUsize;

use clap::Args;
use pwf_client::{
    pb::{
        EffortTier, ListDetail, ListTasksRequest, OrderDirection, OrderField, OrderSpec,
        PriorityTier,
    },
    task::TaskClient,
};
use pwf_models::{
    project::ProjectSelector,
    settings::TaskStatusColors,
    task::{TagInput, TaskTags},
};

use super::{EffortChoice, PriorityChoice, StatusChoice, render::render_list};
use crate::console::Console;

const TASK_LIST_PAGE_SIZE: u32 = 256;

#[derive(Args, Debug)]
pub struct Arguments {
    /// Limit to one project.
    #[arg(long)]
    pub(crate) project: Option<ProjectSelector>,
    #[command(flatten)]
    pub(super) options: Options,
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
}

#[derive(Args, Debug, Clone)]
pub(super) struct Options {
    /// Long form with per-task metadata.
    #[arg(long)]
    pub(crate) long: bool,
    /// List every lifecycle status with no task cap.
    /// An explicit `--status` or `-n` overrides the widened default.
    #[arg(long)]
    pub(crate) all: bool,
    /// Cap to N listed tasks, N >= 1 [default: 10, or unlimited under `--all`].
    #[arg(short = 'n', long, value_name = "N", value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..=100_000))]
    pub(crate) number: Option<usize>,
    /// Sort key `field[:direction]`: created|id|project-id|priority|effort|title,
    /// direction asc|desc. Overrides `default_sort_order` in config.toml (default: id).
    /// Bare created/id/priority sort descending; project-id/effort/title ascending.
    /// Missing effort sorts last.
    /// project-id groups projects, newest ID first within each project.
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
    projects: &pwf_client::project::ProjectClient,
) -> anyhow::Result<String> {
    let options = &arguments.options;
    let project_id = match arguments.project.as_ref() {
        Some(selector) => Some(
            crate::project::resolve_project_id(selector, projects)
                .await?
                .to_string(),
        ),
        None => None,
    };
    let mut request = ListTasksRequest {
        project_id,
        all: options.all,
        number: options
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
        order: options.order,
        status: options.status.map(|status| status.filter() as i32),
        detail: if options.long {
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

/// Parses a `field[:direction]` sort key, using field-specific direction defaults.
pub(super) fn parse_order(
    value: &str,
) -> Result<OrderSpec, pwf_models::task::order::OrderSpecError> {
    use pwf_models::task::order;

    let order: order::OrderSpec = value.parse()?;
    Ok(OrderSpec {
        field: match order.field {
            order::OrderField::Created => OrderField::Created,
            order::OrderField::Id => OrderField::Id,
            order::OrderField::ProjectId => OrderField::ProjectId,
            order::OrderField::Priority => OrderField::Priority,
            order::OrderField::Effort => OrderField::Effort,
            order::OrderField::Title => OrderField::Title,
        } as i32,
        direction: match order.direction {
            order::OrderDirection::Asc => OrderDirection::Asc,
            order::OrderDirection::Desc => OrderDirection::Desc,
        } as i32,
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
