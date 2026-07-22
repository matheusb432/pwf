use clap::Args;
use pwf_application::pending_work::{
    ListResult, ListScope, OrderDirection, OrderField, OrderSpec, ProjectRegistry,
    ProjectResolutionError,
    list::{GetPendingWork, GetPendingWorkError},
};
use pwf_domain::pending_work::{ProjectName, Tags, WorkItemStatusFilter};
use pwf_infra::obsidian::ObsidianStore;

use super::{
    common::{CommonArguments, PendingWorkError, StatusChoice, load_configuration},
    render::render_list,
};
use crate::{config::Config, console::Console};

#[derive(Args, Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "clap mirrors independent command-line switches"
)]
pub struct Arguments {
    /// Limit to one project.
    #[arg(long)]
    pub(crate) project: Option<String>,
    /// Long form with per-item metadata.
    #[arg(long)]
    pub(crate) long: bool,
    /// Show only `## Future` items.
    #[arg(long, conflicts_with_all = ["human", "all"])]
    pub(crate) future: bool,
    /// Show only `## Human` items.
    #[arg(long, conflicts_with_all = ["future", "all"])]
    pub(crate) human: bool,
    /// Include every list section.
    #[arg(long, conflicts_with_all = ["human", "future"])]
    pub(crate) all: bool,
    /// Cap to N listed items (default 10; `-n 0` = all).
    #[arg(short = 'n', long, value_name = "N")]
    pub(crate) number: Option<usize>,
    /// Show only items tagged with this exact effort/complexity tier (1-4).
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=4))]
    pub(crate) effort: Option<u8>,
    /// Discovery tag filter; repeat or comma-separate for several. Every requested tag must
    /// match.
    #[arg(long, allow_hyphen_values = true)]
    pub(crate) tag: Vec<String>,
    /// Sort key: field (created|id|project-id) and/or direction
    /// (asc|desc), each independently optional, in either order. Default:
    /// created desc, flat across every listed project. `project-id`
    /// groups by project (default asc), then newest-id-first within it —
    /// the pre-PWF-0096 default.
    #[arg(short = 'o', long, num_args = 0..=2, value_name = "ORDER", value_parser = ["created", "id", "project-id", "asc", "desc"])]
    pub(crate) order: Vec<String>,
    /// Filter by one lifecycle status, or include every lifecycle status.
    #[arg(long, value_enum, default_value_t = StatusChoice::Active)]
    pub(crate) status: StatusChoice,
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

pub(super) fn run(arguments: &Arguments, console: Console) -> Result<String, PendingWorkError> {
    let configuration = load_configuration(&arguments.common)?;
    let projects = ProjectRegistry::new(configuration.projects.iter().map(|(name, repository)| {
        (
            ProjectName::try_new(name).expect("configured project is non-empty"),
            Some(repository.clone()),
            configuration
                .prefixes
                .get(name)
                .map(|prefix| prefix.to_ascii_uppercase()),
        )
    }));
    let only_project = arguments
        .project
        .as_deref()
        .map(|project| {
            projects
                .resolve(project)
                .map(ToString::to_string)
                .map_err(map_project_resolution_error)
        })
        .transpose()?;
    match arguments.compatibility {
        Compatibility::List => {}
        Compatibility::RejectCreate | Compatibility::RejectCreateAfterProjectResolution => {
            return Err(PendingWorkError::RouteCreateRejected);
        }
    }
    let scope = list_scope_from_flags(arguments.human, arguments.future, arguments.all)?;
    let order = order_spec_from_tokens(&arguments.order)?;
    let tags = tags_from_flags(&arguments.tag)?;
    let parameters = ListParams {
        only_project: only_project.as_deref(),
        long: arguments.long,
        scope,
        number: arguments.number,
        effort: arguments.effort,
        tags: tags.as_ref(),
        order,
        status_filter: arguments.status.filter(),
        color_on: console.color(),
    };
    let store = ObsidianStore::new(configuration.clone());
    let result = pwf_application::pending_work::list::execute(
        &GetPendingWork {
            only_project: parameters.only_project.map(str::to_owned),
            scope: parameters.scope,
            number: parameters.number,
            effort: parameters.effort,
            tags: parameters.tags.cloned(),
            order: parameters.order,
            status_filter: parameters.status_filter,
            include_prerequisite_statuses: parameters.long,
        },
        &store,
        &projects,
    )
    .map_err(map_get_pending_work_error)?;
    Ok(render_query_result(&configuration, parameters, &result))
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
    human: bool,
    future: bool,
    all: bool,
) -> Result<ListScope, PendingWorkError> {
    match (human, future, all) {
        (false, false, false) => Ok(ListScope::Default),
        (true, false, false) => Ok(ListScope::HumanOnly),
        (false, true, false) => Ok(ListScope::FutureOnly),
        (false, false, true) => Ok(ListScope::All),
        _ => Err(PendingWorkError::ConflictingListScopes),
    }
}

fn list_scope_groups_output(scope: ListScope) -> bool {
    matches!(scope, ListScope::All)
}

fn order_field_direction_default(field: OrderField) -> OrderDirection {
    match field {
        OrderField::Created | OrderField::Id => OrderDirection::Desc,
        OrderField::ProjectId => OrderDirection::Asc,
    }
}

/// Parses field and direction tokens in either order, using field-specific defaults.
///
/// # Errors
///
/// Returns [`PendingWorkError`] for conflicting or unknown tokens.
pub(in crate::engines::pending_work) fn order_spec_from_tokens(
    tokens: &[String],
) -> Result<OrderSpec, PendingWorkError> {
    let mut field: Option<(OrderField, &str)> = None;
    let mut direction: Option<(OrderDirection, &str)> = None;
    for token in tokens {
        match token.as_str() {
            t @ ("created" | "id" | "project-id") => {
                if let Some((_, first)) = field {
                    return Err(PendingWorkError::ConflictingOrderField {
                        first: first.to_string(),
                        second: t.to_string(),
                    });
                }
                let parsed = match t {
                    "created" => OrderField::Created,
                    "id" => OrderField::Id,
                    _ => OrderField::ProjectId,
                };
                field = Some((parsed, t));
            }
            t @ ("asc" | "desc") => {
                if let Some((_, first)) = direction {
                    return Err(PendingWorkError::ConflictingOrderDirection {
                        first: first.to_string(),
                        second: t.to_string(),
                    });
                }
                let parsed = if t == "asc" {
                    OrderDirection::Asc
                } else {
                    OrderDirection::Desc
                };
                direction = Some((parsed, t));
            }
            other => {
                return Err(PendingWorkError::BadOrderValue {
                    value: other.to_string(),
                });
            }
        }
    }
    let field = field.map_or(OrderField::Created, |(value, _)| value);
    let direction = direction.map_or(order_field_direction_default(field), |(value, _)| value);
    Ok(OrderSpec { field, direction })
}

#[derive(Clone, Copy)]
pub(in crate::engines::pending_work) struct ListParams<'a> {
    pub only_project: Option<&'a str>,
    pub long: bool,
    pub scope: ListScope,
    pub number: Option<usize>,
    pub effort: Option<u8>,
    pub tags: Option<&'a Tags>,
    pub order: OrderSpec,
    pub status_filter: WorkItemStatusFilter,
    pub color_on: bool,
}

fn render_query_result(cfg: &Config, params: ListParams<'_>, result: &ListResult) -> String {
    render_list(
        result,
        cfg,
        params.only_project,
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
