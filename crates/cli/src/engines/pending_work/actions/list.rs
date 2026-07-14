// Action: list.

use pwf_application::pending_work::list::{GetPendingWork, GetPendingWorkError};
use pwf_domain::pending_work::Tags;
use pwf_infra::obsidian::ObsidianPendingWorkStore;

use super::super::{errors::PendingWorkError, render::render_list};
use crate::config::Config;

const NOTES_DIRECTORY_PREFIX: &str = "Notes directory not found: ";

/// Selects which pending-work sections a list command includes.
///
/// This stays local to the pending-work list action unless another
/// pending-work list caller needs to reason about the selected scope.
///
/// # Examples
///
/// ```ignore
/// let scope = ListScope::All;
/// assert!(matches!(scope, ListScope::All));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::engines::pending_work) enum ListScope {
    Default,
    HumanOnly,
    FutureOnly,
    All,
}

impl ListScope {
    /// Builds a [`ListScope`] from the mutually exclusive list flags.
    ///
    /// # Errors
    ///
    /// Returns [`PendingWorkError::ConflictingListScopes`] when more than one of
    /// `human`, `future`, or `all` is `true`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// assert_eq!(ListScope::from_flags(false, false, false)?, ListScope::Default);
    /// assert_eq!(ListScope::from_flags(true, false, false)?, ListScope::HumanOnly);
    /// assert!(ListScope::from_flags(true, true, false).is_err());
    /// # Ok::<(), PendingWorkError>(())
    /// ```
    pub(in crate::engines::pending_work) fn from_flags(
        human: bool,
        future: bool,
        all: bool,
    ) -> Result<Self, PendingWorkError> {
        match (human, future, all) {
            (false, false, false) => Ok(Self::Default),
            (true, false, false) => Ok(Self::HumanOnly),
            (false, true, false) => Ok(Self::FutureOnly),
            (false, false, true) => Ok(Self::All),
            _ => Err(PendingWorkError::ConflictingListScopes),
        }
    }

    fn groups_output(self) -> bool {
        matches!(self, Self::All)
    }

    pub(in crate::engines::pending_work) fn to_domain(self) -> pwf_domain::pending_work::ListScope {
        match self {
            Self::Default => pwf_domain::pending_work::ListScope::Default,
            Self::HumanOnly => pwf_domain::pending_work::ListScope::HumanOnly,
            Self::FutureOnly => pwf_domain::pending_work::ListScope::FutureOnly,
            Self::All => pwf_domain::pending_work::ListScope::All,
        }
    }
}

/// `-o`/`--order` sort field (default [`OrderField::Created`]). `Created`/`Id`
/// sort flat across every listed project (no project grouping); `ProjectId` is
/// the explicit opt-in that reproduces the pre-PWF-0096 default — project-name
/// ascending, then newest-id-first within each project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::engines::pending_work) enum OrderField {
    Created,
    Id,
    ProjectId,
}

/// `-o`/`--order` sort direction. Its default depends on the field —
/// [`OrderField::default_direction`] — since "newest first" (`Desc`) is the
/// intuitive default for `created`/`id`, while `project-id` defaults to `Asc`
/// (project-name ascending) to match the pre-PWF-0096 convention it reproduces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::engines::pending_work) enum OrderDirection {
    Asc,
    Desc,
}

impl OrderField {
    /// The direction assumed when `--order`'s tokens name this field but no
    /// direction keyword.
    fn default_direction(self) -> OrderDirection {
        match self {
            OrderField::Created | OrderField::Id => OrderDirection::Desc,
            OrderField::ProjectId => OrderDirection::Asc,
        }
    }

    fn to_domain(self) -> pwf_domain::pending_work::OrderField {
        match self {
            Self::Created => pwf_domain::pending_work::OrderField::Created,
            Self::Id => pwf_domain::pending_work::OrderField::Id,
            Self::ProjectId => pwf_domain::pending_work::OrderField::ProjectId,
        }
    }
}

impl OrderDirection {
    fn to_domain(self) -> pwf_domain::pending_work::OrderDirection {
        match self {
            Self::Asc => pwf_domain::pending_work::OrderDirection::Asc,
            Self::Desc => pwf_domain::pending_work::OrderDirection::Desc,
        }
    }
}

/// `pwf list -o`/`--order`'s resolved sort key: which field, and which direction.
/// Default is `Created` + `Desc` (newest-created-first, flat across every listed
/// project); `--order project-id` reproduces the pre-PWF-0096 grouped ordering
/// (project-name ascending, then newest-id-first within a project).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::engines::pending_work) struct OrderSpec {
    pub(in crate::engines::pending_work) field: OrderField,
    pub(in crate::engines::pending_work) direction: OrderDirection,
}

impl Default for OrderSpec {
    fn default() -> Self {
        Self {
            field: OrderField::Created,
            direction: OrderField::Created.default_direction(),
        }
    }
}

impl OrderSpec {
    /// Builds an [`OrderSpec`] from `--order`'s raw tokens. Each token is
    /// independently a field (`created`/`id`/`project-id`) or a direction
    /// (`asc`/`desc`) keyword, in either order; a missing field defaults to
    /// `created`, and a missing direction defaults per the resolved field
    /// ([`OrderField::default_direction`]).
    ///
    /// # Errors
    ///
    /// Returns a `PendingWorkError` when two tokens name the same category
    /// (e.g. `--order created id` or `--order asc desc`), or a token is neither.
    pub(in crate::engines::pending_work) fn from_tokens(
        tokens: &[String],
    ) -> Result<Self, PendingWorkError> {
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
        let direction = direction.map_or(field.default_direction(), |(value, _)| value);
        Ok(Self { field, direction })
    }

    pub(in crate::engines::pending_work) fn to_domain(self) -> pwf_domain::pending_work::OrderSpec {
        pwf_domain::pending_work::OrderSpec {
            field: self.field.to_domain(),
            direction: self.direction.to_domain(),
        }
    }
}

/// The selection + rendering options for a list run. Groups the arguments that overflowed
/// the list-run helper signature so each call site names its intent.
#[derive(Clone, Copy)]
pub(in crate::engines::pending_work) struct ListParams<'a> {
    pub only_project: Option<&'a str>,
    pub long: bool,
    pub scope: ListScope,
    pub number: Option<usize>,
    pub effort: Option<u8>,
    pub tags: Option<&'a Tags>,
    pub order: OrderSpec,
    pub color_on: bool,
}

/// Shared list query composition for `pwf list` and the hidden `route` word-router.
pub(in crate::engines::pending_work) fn run_list_query(
    cfg: &Config,
    params: ListParams<'_>,
) -> Result<String, PendingWorkError> {
    let store = ObsidianPendingWorkStore::new(cfg.clone());
    let result = pwf_application::pending_work::list::execute(
        GetPendingWork {
            only_project: params.only_project.map(str::to_owned),
            scope: params.scope.to_domain(),
            number: params.number,
            effort: params.effort,
            tags: params.tags.cloned(),
            order: params.order.to_domain(),
        },
        &store,
    )
    .map_err(map_get_pending_work_error)?;
    Ok(render_query_result(cfg, params, &result))
}

pub(in crate::engines::pending_work) fn render_query_result(
    cfg: &Config,
    params: ListParams<'_>,
    result: &pwf_domain::pending_work::ListResult,
) -> String {
    render_list(
        result,
        cfg,
        params.only_project,
        params.long,
        params.scope.groups_output(),
        params.color_on,
    )
}

pub(in crate::engines::pending_work) fn map_get_pending_work_error(
    error: GetPendingWorkError,
) -> PendingWorkError {
    match error {
        GetPendingWorkError::ReadStore(source) => {
            let message = source.to_string();
            if let Some(path) = message.strip_prefix(NOTES_DIRECTORY_PREFIX) {
                PendingWorkError::NotesDirectoryNotFound {
                    path: path.to_string(),
                }
            } else {
                PendingWorkError::ApplicationList(message)
            }
        }
        invalid @ (GetPendingWorkError::InvalidTags { .. }
        | GetPendingWorkError::InvalidProject { .. }) => {
            PendingWorkError::ApplicationList(invalid.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use pwf_domain::pending_work::Tags;

    use super::*;
    use crate::engines::pending_work::errors::PendingWorkError;

    #[test]
    fn missing_notes_dir_returns_typed_error_with_legacy_display() {
        let guard = tempfile::tempdir().unwrap();
        let missing = guard.path().join("missing_notes");
        assert!(!missing.exists());
        let missing = missing.to_string_lossy().into_owned();
        let missing_json = serde_json::to_string(&missing).unwrap();
        let cfg = crate::config::from_json(
            &format!(
                r#"{{ "notesDir": {missing_json}, "projects": {{ "pwf": "/repo" }}, "prefixes": {{ "pwf": "PWF" }} }}"#
            ),
            None,
        )
        .unwrap();

        let err = run_list_query(
            &cfg,
            ListParams {
                only_project: None,
                long: false,
                scope: ListScope::Default,
                number: None,
                effort: None,
                tags: None,
                order: OrderSpec::default(),
                color_on: false,
            },
        )
        .unwrap_err();

        assert_matches!(
            err,
            PendingWorkError::NotesDirectoryNotFound { ref path }
                if path == &missing
        );
        assert_eq!(
            err.to_string(),
            format!("Notes directory not found: {missing}")
        );
    }

    #[test]
    fn invalid_application_tags_map_to_list_error() {
        let error = GetPendingWorkError::InvalidTags {
            id: "PWF-0001".to_string(),
            source: Tags::parse_frontmatter("sqlite, godot").unwrap_err(),
        };

        let got = map_get_pending_work_error(error);

        assert_matches!(got, PendingWorkError::ApplicationList(_));
        assert_eq!(
            got.to_string(),
            "item PWF-0001 has invalid tags frontmatter: invalid tags frontmatter: \"sqlite, godot\""
        );
    }

    #[test]
    fn from_tokens_project_id_alone_defaults_direction_to_asc() {
        let spec = OrderSpec::from_tokens(&["project-id".to_string()]).unwrap();
        assert_eq!(spec.field, OrderField::ProjectId);
        assert_eq!(spec.direction, OrderDirection::Asc);
    }

    #[test]
    fn cli_scope_and_order_convert_to_application_contract_types() {
        let order = OrderSpec {
            field: OrderField::ProjectId,
            direction: OrderDirection::Desc,
        };

        assert_eq!(
            ListScope::FutureOnly.to_domain(),
            pwf_domain::pending_work::ListScope::FutureOnly
        );
        assert_eq!(
            order.to_domain(),
            pwf_domain::pending_work::OrderSpec {
                field: pwf_domain::pending_work::OrderField::ProjectId,
                direction: pwf_domain::pending_work::OrderDirection::Desc,
            }
        );
    }

    #[test]
    fn order_spec_defaults_to_created_desc_when_no_tokens() {
        let spec = OrderSpec::from_tokens(&[]).unwrap();
        assert_eq!(spec, OrderSpec::default());
        assert_eq!(spec.field, OrderField::Created);
        assert_eq!(spec.direction, OrderDirection::Desc);
    }

    #[test]
    fn order_spec_field_only_defaults_direction_to_desc() {
        let spec = OrderSpec::from_tokens(&["id".to_string()]).unwrap();
        assert_eq!(spec.field, OrderField::Id);
        assert_eq!(spec.direction, OrderDirection::Desc);

        let spec = OrderSpec::from_tokens(&["created".to_string()]).unwrap();
        assert_eq!(spec.field, OrderField::Created);
        assert_eq!(spec.direction, OrderDirection::Desc);
    }

    #[test]
    fn order_spec_direction_only_defaults_field_to_created() {
        let spec = OrderSpec::from_tokens(&["asc".to_string()]).unwrap();
        assert_eq!(spec.field, OrderField::Created);
        assert_eq!(spec.direction, OrderDirection::Asc);
    }

    #[test]
    fn order_spec_accepts_field_and_direction_in_either_order() {
        let a = OrderSpec::from_tokens(&["id".to_string(), "asc".to_string()]).unwrap();
        let b = OrderSpec::from_tokens(&["asc".to_string(), "id".to_string()]).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.field, OrderField::Id);
        assert_eq!(a.direction, OrderDirection::Asc);
    }

    #[test]
    fn order_spec_rejects_duplicate_field_tokens() {
        let err = OrderSpec::from_tokens(&["created".to_string(), "id".to_string()]).unwrap_err();
        assert_matches!(
            err,
            PendingWorkError::ConflictingOrderField { ref first, ref second }
                if first == "created" && second == "id"
        );
    }

    #[test]
    fn order_spec_rejects_duplicate_direction_tokens() {
        let err = OrderSpec::from_tokens(&["asc".to_string(), "desc".to_string()]).unwrap_err();
        assert_matches!(
            err,
            PendingWorkError::ConflictingOrderDirection { ref first, ref second }
                if first == "asc" && second == "desc"
        );
    }

    #[test]
    fn order_spec_rejects_unknown_token() {
        let err = OrderSpec::from_tokens(&["bogus".to_string()]).unwrap_err();
        assert_matches!(
            err,
            PendingWorkError::BadOrderValue { ref value } if value == "bogus"
        );
    }
}
