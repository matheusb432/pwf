use clap::Args;
use pwf_application::pending_work::{
    ListResult, ListScope, OrderDirection, OrderField, OrderSpec, ProjectRegistry,
    ProjectResolutionError,
    list::{GetPendingWork, GetPendingWorkError},
};
use pwf_domain::pending_work::{Tags, WorkItemStatusFilter};
use pwf_infra::obsidian::ObsidianStore;

use super::{
    common::{CommonArguments, PendingWorkError, SectionChoice, StatusChoice},
    render::render_list,
};
use crate::console::Console;

const LIST_CAP_DEFAULT: usize = 10;

/// Order used by the `pwf <project>` route: grouped by project, newest id first.
pub(in crate::engines::pending_work) const PROJECT_GROUPED_ORDER: OrderSpec = OrderSpec {
    field: OrderField::ProjectId,
    direction: OrderDirection::Asc,
};

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
    /// Show only items tagged with this exact effort/complexity tier (1-4).
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=4))]
    pub(crate) effort: Option<u8>,
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
    let only_project = arguments
        .project
        .as_deref()
        .map(|project| {
            projects
                .resolve(project)
                .cloned()
                .map_err(map_project_resolution_error)
        })
        .transpose()?;
    let location = only_project
        .as_ref()
        .map(|project| {
            store
                .tasks_path(project)
                .map(|path| path.display().to_string())
                .map_err(|error| PendingWorkError::ApplicationRead(error.to_string()))
        })
        .transpose()?
        .unwrap_or_else(|| "managed project task paths".to_string());
    match arguments.compatibility {
        Compatibility::List => {}
        Compatibility::RejectCreate | Compatibility::RejectCreateAfterProjectResolution => {
            return Err(PendingWorkError::RouteCreateRejected);
        }
    }
    let scope = list_scope_from_flags(arguments.section, arguments.all);
    let order = arguments.order.unwrap_or_default();
    let tags = tags_from_flags(&arguments.tag)?;
    let (status, cap) = effective_status_and_cap(arguments.all, arguments.status, arguments.number);
    let parameters = ListParams {
        only_project: only_project.as_ref().map(AsRef::as_ref),
        long: arguments.long,
        scope,
        cap,
        effort: arguments.effort,
        tags: tags.as_ref(),
        order,
        status_filter: status.filter(),
        color_on: console.color(),
    };
    let result = pwf_application::pending_work::list::execute(
        &GetPendingWork {
            only_project: parameters.only_project.map(str::to_owned),
            scope: parameters.scope,
            cap: parameters.cap,
            effort: parameters.effort,
            tags: parameters.tags.cloned(),
            order: parameters.order,
            status_filter: parameters.status_filter,
            include_prerequisite_statuses: parameters.long,
        },
        store,
        projects,
    )
    .map_err(map_get_pending_work_error)?;
    Ok(render_query_result(&location, parameters, &result))
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

fn tags_from_flags(values: &[String]) -> Result<Option<Tags>, PendingWorkError> {
    if values.is_empty() {
        return Ok(None);
    }
    Tags::parse_values(values)
        .map(Some)
        .map_err(|error| PendingWorkError::InvalidTag {
            raw: error.raw().to_string(),
        })
}

pub(in crate::engines::pending_work) fn list_scope_from_flags(
    section: Option<SectionChoice>,
    all: bool,
) -> ListScope {
    if all {
        return ListScope::All;
    }
    match section {
        None => ListScope::Default,
        Some(SectionChoice::Future) => ListScope::FutureOnly,
        Some(SectionChoice::Human) => ListScope::HumanOnly,
    }
}

fn list_scope_groups_output(scope: ListScope) -> bool {
    matches!(scope, ListScope::All)
}

/// `--all` widens the defaults to every lifecycle status and no item cap;
/// an explicit `--status` or `-n` wins over the widened default.
fn effective_status_and_cap(
    all: bool,
    status: Option<StatusChoice>,
    number: Option<usize>,
) -> (StatusChoice, Option<usize>) {
    let status = status.unwrap_or(if all {
        StatusChoice::All
    } else {
        StatusChoice::Active
    });
    let cap = number.or((!all).then_some(LIST_CAP_DEFAULT));
    (status, cap)
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

#[derive(Clone, Copy)]
pub(in crate::engines::pending_work) struct ListParams<'a> {
    pub only_project: Option<&'a str>,
    pub long: bool,
    pub scope: ListScope,
    pub cap: Option<usize>,
    pub effort: Option<u8>,
    pub tags: Option<&'a Tags>,
    pub order: OrderSpec,
    pub status_filter: WorkItemStatusFilter,
    pub color_on: bool,
}

fn render_query_result(location: &str, params: ListParams<'_>, result: &ListResult) -> String {
    render_list(
        result,
        location,
        params.status_filter,
        params.long,
        list_scope_groups_output(params.scope),
        params.color_on,
    )
}

fn map_get_pending_work_error(error: GetPendingWorkError) -> PendingWorkError {
    let message = match error {
        GetPendingWorkError::ReadStore(source) => source.to_string(),
        invalid @ (GetPendingWorkError::InvalidTags { .. }
        | GetPendingWorkError::InvalidProject { .. }) => invalid.to_string(),
    };
    PendingWorkError::ApplicationList(message)
}

#[cfg(test)]
mod tests {
    use pwf_domain::pending_work::{WorkItemStatus, WorkItemStatusFilter};

    use super::*;

    #[test]
    fn all_widens_status_and_uncaps() {
        let (status, cap) = effective_status_and_cap(true, None, None);
        assert_eq!(status.filter(), WorkItemStatusFilter::All);
        assert_eq!(cap, None);
    }

    #[test]
    fn explicit_status_and_cap_win_over_all() {
        let (status, cap) = effective_status_and_cap(true, Some(StatusChoice::Done), Some(5));
        assert_eq!(
            status.filter(),
            WorkItemStatusFilter::Exact(WorkItemStatus::Done)
        );
        assert_eq!(cap, Some(5));
    }

    #[test]
    fn defaults_without_all_stay_active_and_capped() {
        let (status, cap) = effective_status_and_cap(false, None, None);
        assert_eq!(
            status.filter(),
            WorkItemStatusFilter::Exact(WorkItemStatus::Active)
        );
        assert_eq!(cap, Some(LIST_CAP_DEFAULT));
    }

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
