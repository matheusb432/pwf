use clap::Args;
use pwf_application::pending_work::{
    ListMode, ListSection, OrderDirection, OrderField, OrderSpec, ProjectRegistry,
    ProjectResolutionError,
    get_pending_work::{self, GetPendingWork, GetPendingWorkError},
};
use pwf_infra::obsidian::ObsidianStore;

use super::{
    common::{CommonArguments, EffortChoice, PendingWorkError, SectionChoice, StatusChoice},
    render::render_list,
};
use crate::console::Console;

#[derive(Args, Debug)]
pub struct Arguments {
    /// Limit to one project.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Long form with per-item metadata.
    #[arg(long)]
    pub(crate) long: bool,
    /// Show only this scoped section.
    #[arg(long, value_enum, conflicts_with = "all")]
    pub(crate) section: Option<SectionChoice>,
    /// List everything: every section, every lifecycle status, no item cap.
    /// An explicit `--status` or `-n` overrides the widened default.
    #[arg(long)]
    pub(crate) all: bool,
    /// Cap to N listed items, N >= 1 [default: 10, or unlimited under `--all`].
    #[arg(short = 'n', long, value_name = "N", value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..=100_000))]
    pub(crate) number: Option<usize>,
    /// Show only items tagged with this exact effort/complexity tier.
    #[arg(long, value_enum)]
    pub(crate) effort: Option<EffortChoice>,
    /// Discovery tag filter; repeat or comma-separate for several. Every requested tag must
    /// match.
    #[arg(long, allow_hyphen_values = true)]
    pub(crate) tag: Vec<String>,
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
    #[command(flatten)]
    pub(crate) common: CommonArguments,
    #[arg(skip)]
    pub(crate) compatibility: Compatibility,
    #[arg(skip)]
    pub(crate) mode: ListMode,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum Compatibility {
    #[default]
    List,
    RejectCreate,
    RejectCreateAfterProjectResolution,
}

pub(super) fn run(
    arguments: &Arguments,
    console: Console,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
) -> Result<String, PendingWorkError> {
    match arguments.compatibility {
        Compatibility::List => {}
        Compatibility::RejectCreate => {
            return Err(PendingWorkError::RouteCreateRejected);
        }
        Compatibility::RejectCreateAfterProjectResolution => {
            if let Some(project) = arguments.project.as_deref() {
                projects
                    .resolve(project)
                    .map_err(map_project_resolution_error)?;
            }
            return Err(PendingWorkError::RouteCreateRejected);
        }
    }
    let result = get_pending_work::execute(
        &GetPendingWork {
            project_identifier: arguments.project.clone(),
            section: arguments.section.map(|section| match section {
                SectionChoice::Future => ListSection::Future,
                SectionChoice::Human => ListSection::Human,
            }),
            all: arguments.all,
            number: arguments.number,
            effort: arguments.effort.map(Into::into),
            tags: arguments.tag.clone(),
            order: arguments.order,
            status: arguments.status.map(StatusChoice::filter),
            include_prerequisite_statuses: arguments.long,
            mode: arguments.mode,
        },
        store,
        projects,
    )
    .map_err(map_get_pending_work_error)?;
    let location = result
        .project
        .as_ref()
        .map(|project| {
            store
                .tasks_path(project)
                .map(|path| path.display().to_string())
                .map_err(|error| PendingWorkError::ApplicationRead(error.to_string()))
        })
        .transpose()?
        .unwrap_or_else(|| "managed project task paths".to_string());
    Ok(render_list(
        &result,
        &location,
        result.status_filter,
        arguments.long,
        result.grouped,
        console.color(),
    ))
}

fn map_project_resolution_error(error: ProjectResolutionError) -> PendingWorkError {
    match error {
        ProjectResolutionError::Unknown { identifier, known } => {
            PendingWorkError::UnknownManagedProject { identifier, known }
        }
        ProjectResolutionError::Ambiguous {
            identifier,
            matches,
        } => PendingWorkError::AmbiguousManagedProject {
            identifier,
            matches,
        },
        other => PendingWorkError::ApplicationRead(other.to_string()),
    }
}

fn order_field_direction_default(field: OrderField) -> OrderDirection {
    match field {
        OrderField::Created | OrderField::Id => OrderDirection::Desc,
        OrderField::ProjectId => OrderDirection::Asc,
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
        None => order_field_direction_default(field),
        Some("asc") => OrderDirection::Asc,
        Some("desc") => OrderDirection::Desc,
        Some(_) => return Err(USAGE.to_string()),
    };
    Ok(OrderSpec { field, direction })
}

fn map_get_pending_work_error(error: GetPendingWorkError) -> PendingWorkError {
    match error {
        GetPendingWorkError::ResolveProject(error) => map_project_resolution_error(error),
        GetPendingWorkError::InvalidRequestedTags(error) => PendingWorkError::InvalidTag {
            raw: error.raw().to_string(),
        },
        GetPendingWorkError::ReadStore(source) => {
            PendingWorkError::ApplicationList(source.to_string())
        }
        invalid @ GetPendingWorkError::InvalidTags { .. } => {
            PendingWorkError::ApplicationList(invalid.to_string())
        }
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
