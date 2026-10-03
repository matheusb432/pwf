use std::{fmt::Write as _, num::NonZeroUsize};

use clap::Args;
use pwf_client::{
    pb::{
        EffortTier, ListDetail, ListTasksRequest, OrderDirection, OrderField, OrderSpec,
        PriorityTier,
    },
    task::TaskClient,
};
use pwf_models::{
    project::ProjectId,
    settings::UserSettings,
    task::{TagInput, TaskTags},
};

use super::{
    ContentFormat, ContentSelection, EffortChoice, PriorityChoice, StatusChoice,
    render::render_list,
};
use crate::console::Console;

const TASK_LIST_PAGE_SIZE: u32 = 256;

#[derive(Args, Debug)]
pub struct Arguments {
    /// Limit to one project.
    #[arg(long, value_parser = crate::project::parse_project_id)]
    pub(crate) project: Option<ProjectId>,
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
    /// Full content: rich on terminals, md when redirected; choose rich, md, or json explicitly.
    #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "", value_parser = ContentSelection::parse, value_name = "rich|md|json")]
    pub(crate) long: Option<ContentSelection>,
    /// List every lifecycle status with no task cap.
    /// An explicit `--status` or `-n` overrides the widened default.
    #[arg(long)]
    pub(crate) all: bool,
    /// Cap to N listed tasks. [default: `default_list_page_size` setting (10), unlimited under
    /// `--all`].
    #[arg(short = 'n', long, value_name = "N", value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..=100_000))]
    pub(crate) number: Option<usize>,
    /// Sort key `field[:direction]`: created|id|project-id|priority|effort|title,
    /// direction asc|desc.
    #[arg(short = 'o', long, value_name = "FIELD[:DIR]", value_parser = parse_order)]
    pub(crate) order: Option<OrderSpec>,
    /// Filter by one lifecycle status, or include every lifecycle status
    /// [default: active, or all under `--all`].
    #[arg(short = 's', long, value_enum)]
    pub(crate) status: Option<StatusChoice>,
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    settings: &UserSettings,
    client: &TaskClient,
    projects: &pwf_client::project::ProjectClient,
) -> Result<String, crate::error::Error> {
    let options = &arguments.options;
    let project_id = match arguments.project.as_ref() {
        Some(id) => Some(
            crate::project::resolve_project_id(id, projects)
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
        detail: if options.long.is_some() {
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
    let format = options.long.map(|selection| selection.resolve(console));
    let mut result = client.list_tasks(request.clone()).await?;
    let hidden = result.hidden;
    let mut output = String::new();
    let mut tasks_rendered = false;
    let mut seen_tokens = std::collections::HashSet::new();
    loop {
        result.hidden = 0;
        append_page(
            &mut output,
            &result,
            format,
            settings,
            console.color(),
            console.stdout_columns(),
            tasks_rendered,
        )?;
        tasks_rendered |= !result.tasks.is_empty();
        let Some(token) = result.next_page_token.take() else {
            break;
        };
        if !seen_tokens.insert(token.clone()) {
            return Err(
                anyhow::anyhow!("pwf-server repeated a task-list continuation token").into(),
            );
        }
        request.page_token = Some(token);
        // Release the full record page before decoding its successor.
        drop(result);
        result = client.list_tasks(request.clone()).await?;
    }
    if format == Some(ContentFormat::Json) {
        if tasks_rendered {
            output.push_str("\n]");
        } else {
            output.push_str("[]");
        }
    }
    if tasks_rendered && hidden > 0 && matches!(format, None | Some(ContentFormat::Rich)) {
        if format == Some(ContentFormat::Rich) || !output.ends_with('\n') {
            output.push('\n');
        }
        let _ = write!(
            output,
            "... and {hidden} more; use '--all' to list everything"
        );
    }
    Ok(output)
}

fn append_page(
    output: &mut String,
    result: &pwf_client::pb::ListTasksResponse,
    format: Option<ContentFormat>,
    settings: &UserSettings,
    color: bool,
    columns: Option<usize>,
    tasks_rendered: bool,
) -> anyhow::Result<()> {
    if tasks_rendered && result.tasks.is_empty() {
        return Ok(());
    }
    let location = result.project_task_path.as_ref().map_or_else(
        || "managed project task paths".to_string(),
        ToString::to_string,
    );
    match format {
        Some(ContentFormat::Json) => {
            if !result.tasks.is_empty() {
                let page = super::render::content::json_list(&result.tasks)?;
                let items = page
                    .strip_prefix("[\n")
                    .and_then(|page| page.strip_suffix("\n]"))
                    .ok_or_else(|| {
                        anyhow::anyhow!("Task-list JSON omitted its array delimiters")
                    })?;
                output.push_str(if tasks_rendered { ",\n" } else { "[\n" });
                output.push_str(items);
            }
        }
        Some(ContentFormat::Md) => {
            for (index, task) in result.tasks.iter().enumerate() {
                let source = task.source.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("pwf-server omitted the task file for {}", task.id)
                })?;
                if tasks_rendered || index > 0 {
                    output.push('\n');
                }
                output.push_str(source);
            }
        }
        Some(ContentFormat::Rich) => {
            if tasks_rendered && !result.tasks.is_empty() {
                if !output.ends_with('\n') {
                    output.push('\n');
                }
                output.push('\n');
            }
            output.push_str(&super::render::content::rich_list(
                result, settings, color, columns,
            )?);
        }
        None => {
            if tasks_rendered && !result.tasks.is_empty() {
                output.push('\n');
            }
            output.push_str(&render_list(result, &location, settings, color));
        }
    }
    Ok(())
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
    fn rendering_pages_preserves_separators_and_json_indentation() {
        use pwf_client::pb::{ListTasksResponse, ListedTask, TaskStatus, TaskStatusFilter};

        let result = ListTasksResponse {
            tasks: [
                "body without newline",
                "body with newline\n",
                "\r\n  final body  \r\n",
            ]
            .into_iter()
            .enumerate()
            .map(|(index, body)| ListedTask {
                id: format!("FOO-{:04}", index + 1),
                heading: "sample task".into(),
                project: "foo".into(),
                status: TaskStatus::Active as i32,
                file_path: format!("/tasks/FOO-{:04}.md", index + 1),
                body: body.into(),
                source: Some(format!("---\r\nid: FOO-{:04}\r\n---\r\n{body}", index + 1)),
                ..Default::default()
            })
            .collect(),
            status_filter: TaskStatusFilter::All as i32,
            ..Default::default()
        };
        let settings = UserSettings::default();
        for format in [
            None,
            Some(ContentFormat::Md),
            Some(ContentFormat::Json),
            Some(ContentFormat::Rich),
        ] {
            let expected = match format {
                None => render_list(&result, "managed project task paths", &settings, false),
                Some(ContentFormat::Md) => result
                    .tasks
                    .iter()
                    .map(|task| task.source.as_deref().unwrap())
                    .collect::<Vec<_>>()
                    .join("\n"),
                Some(ContentFormat::Json) => {
                    super::super::render::content::json_list(&result.tasks).unwrap()
                }
                Some(ContentFormat::Rich) => {
                    super::super::render::content::rich_list(&result, &settings, false, None)
                        .unwrap()
                }
            };
            let mut actual = String::new();
            for (index, task) in result.tasks.iter().enumerate() {
                let page = ListTasksResponse {
                    tasks: vec![task.clone()],
                    ..result.clone()
                };
                append_page(
                    &mut actual,
                    &page,
                    format,
                    &settings,
                    false,
                    None,
                    index > 0,
                )
                .unwrap();
            }
            if format == Some(ContentFormat::Json) {
                actual.push_str("\n]");
            }
            assert_eq!(actual, expected, "{format:?}");
        }
    }

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
