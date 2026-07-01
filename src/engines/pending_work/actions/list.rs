// Action: list.

use super::super::{
    domain::read_models::ListResult, effort::EffortTier, errors::PendingWorkError, model::Item,
    query::get_pending_work, render::render_list,
};
use crate::config::Config;

/// Default item cap for `pw list` when `-n` is absent; keeps agents from being
/// flooded with tokens (PWF-0020). `-n 0` overrides to unlimited.
const DEFAULT_LIST_CAP: usize = 10;

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

    fn includes(self, section: Option<&str>) -> bool {
        match self {
            Self::Default => section.is_none(),
            Self::HumanOnly => matches!(section, Some("Human")),
            Self::FutureOnly => matches!(section, Some("Future")),
            Self::All => true,
        }
    }

    fn groups_output(self) -> bool {
        matches!(self, Self::All)
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
    /// `created`, and a missing direction defaults per the *resolved* field
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
        let field = field.map_or(OrderField::Created, |(f, _)| f);
        let direction = direction.map_or(field.default_direction(), |(d, _)| d);
        Ok(Self { field, direction })
    }
}

fn section_group_rank(section: Option<&str>) -> u8 {
    match section {
        None => 0,
        Some("Low-prio") => 1,
        Some("Human") => 2,
        Some("Future") => 3,
        Some(_) => 4,
    }
}

/// `true` when `wanted` is `None` (no filter), or `item.effort` parses to exactly
/// `wanted`. A hand-corrupted/unparseable `effort:` value never matches a filter.
fn effort_matches(item: &Item, wanted: Option<u8>) -> bool {
    let Some(wanted) = wanted else { return true };
    item.effort
        .as_deref()
        .and_then(EffortTier::parse)
        .map(u8::from)
        == Some(wanted)
}

/// Numeric ID suffix (digits after the last `-`), or 0 when unparseable. Zero-padded
/// per-prefix counters mean higher = newer inside a project group.
fn id_suffix(id: &str) -> u64 {
    id.rsplit_once('-')
        .and_then(|(_, n)| n.parse().ok())
        .unwrap_or(0)
}

/// An item's `created:` frontmatter value, or `""` when absent (legacy inline
/// items, or a hand-corrupted note) — sorts first ascending / last descending.
fn created_key(item: &Item) -> &str {
    item.created.as_deref().unwrap_or("")
}

/// Order comparison for `order.field`. `Created`/`Id` are flat across every
/// listed project: computed ascending, tiebroken by the full id string (also
/// ascending), then flipped whole when `order.direction` is `Desc` — flipping
/// the *entire* chain (not just the primary key) keeps the tiebreak
/// deterministic in the requested direction. `ProjectId` groups by project
/// instead: `order.direction` controls the project axis only (default `Desc`
/// still means "reverse of ascending", i.e. Z-A), while the id sub-key stays
/// fixed newest-first — reproducing the pre-PWF-0096 default exactly.
fn item_order_cmp(order: OrderSpec, a: &Item, b: &Item) -> std::cmp::Ordering {
    if order.field == OrderField::ProjectId {
        let project_cmp = match order.direction {
            OrderDirection::Asc => a.project.cmp(&b.project),
            OrderDirection::Desc => b.project.cmp(&a.project),
        };
        return project_cmp
            .then_with(|| id_suffix(&b.id).cmp(&id_suffix(&a.id)))
            .then_with(|| b.id.cmp(&a.id));
    }
    let ascending = match order.field {
        OrderField::Created => created_key(a).cmp(created_key(b)),
        OrderField::Id => id_suffix(&a.id).cmp(&id_suffix(&b.id)),
        OrderField::ProjectId => unreachable!("handled above"),
    }
    .then_with(|| a.id.cmp(&b.id));
    match order.direction {
        OrderDirection::Asc => ascending,
        OrderDirection::Desc => ascending.reverse(),
    }
}

/// Reorder items by `order` — flat across every listed project for
/// `Created`/`Id`; grouped by project for the explicit `ProjectId` opt-in
/// (the pre-PWF-0096 default).
fn sort_by_order(items: &mut [Item], order: OrderSpec) {
    items.sort_by(|a, b| item_order_cmp(order, a, b));
}

fn sort_by_group_then_order(items: &mut [Item], order: OrderSpec) {
    items.sort_by(|a, b| {
        section_group_rank(a.section.as_deref())
            .cmp(&section_group_rank(b.section.as_deref()))
            .then_with(|| item_order_cmp(order, a, b))
    });
}

/// Keep at most `cap` items (`cap == 0` means unlimited). Returns `(kept, hidden)`;
/// `kept.len() + hidden` always equals the input length.
fn apply_cap(items: Vec<Item>, cap: usize) -> (Vec<Item>, usize) {
    if cap == 0 || items.len() <= cap {
        return (items, 0);
    }
    let hidden = items.len() - cap;
    let mut kept = items;
    kept.truncate(cap);
    (kept, hidden)
}

/// List action implementation. Scoped sections are hidden unless a list scope
/// flag re-includes them.
pub(in crate::engines::pending_work) fn run_list_action(
    cfg: &Config,
    only_project: Option<&str>,
    long: bool,
    scope: ListScope,
    number: Option<usize>,
    effort: Option<u8>,
    order: OrderSpec,
) -> Result<String, PendingWorkError> {
    let mut items: Vec<_> = get_pending_work(cfg, only_project)?
        .into_iter()
        .filter(|i| scope.includes(i.section.as_deref()))
        .filter(|i| effort_matches(i, effort))
        .collect();
    // Order + cap before rendering so the selected/capped sequence is consistent.
    if scope.groups_output() {
        sort_by_group_then_order(&mut items, order);
    } else {
        sort_by_order(&mut items, order);
    }
    let (items, hidden) = apply_cap(items, number.unwrap_or(DEFAULT_LIST_CAP));
    let result = ListResult::from_items(items, hidden);
    Ok(render_list(
        &result,
        cfg,
        only_project,
        long,
        scope.groups_output(),
    ))
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;
    use crate::engines::pending_work::errors::PendingWorkError;

    /// Minimal `Item` carrying only the `id` the pure helpers read.
    fn item(id: &str) -> Item {
        Item {
            id: id.to_string(),
            project: "glep-shimeji".to_string(),
            session: "t".to_string(),
            prompt: String::new(),
            repo: None,
            note: String::new(),
            item_file: None,
            line: 0,
            format: String::new(),
            marker_index: 0,
            marker_length: 0,
            launchable: true,
            needs_prompt: false,
            issues: vec![],
            section: None,
            prereq: None,
            effort: None,
            created: None,
        }
    }

    fn ids(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.id.as_str()).collect()
    }

    fn unique_missing_notes_dir(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("pwf_list_{name}_{nanos}"))
    }

    #[test]
    fn missing_notes_dir_returns_typed_error_with_legacy_display() {
        let missing = unique_missing_notes_dir("missing_notes");
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

        let err = run_list_action(
            &cfg,
            None,
            false,
            ListScope::Default,
            None,
            None,
            OrderSpec::default(),
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

    fn dated(id: &str, created: &str) -> Item {
        let mut i = item(id);
        i.created = Some(created.to_string());
        i
    }

    #[test]
    fn sort_by_order_id_is_flat_across_projects() {
        // OrderField::Id is not grouped by project: "config-handler" sorts
        // before "pwf" alphabetically, but the higher id suffix (PWF-0099)
        // still wins under a flat id-desc order — proving project plays no
        // part, unlike the ProjectId opt-in.
        let mut cfg_item = item("CFG-0001");
        cfg_item.project = "config-handler".to_string();
        let mut pwf_item = item("PWF-0099");
        pwf_item.project = "pwf".to_string();

        let order = OrderSpec {
            field: OrderField::Id,
            direction: OrderDirection::Desc,
        };
        let mut items = vec![cfg_item, pwf_item];
        sort_by_order(&mut items, order);
        assert_eq!(ids(&items), ["PWF-0099", "CFG-0001"]);
    }

    #[test]
    fn sort_by_order_project_id_reproduces_legacy_grouped_ordering() {
        let mut cfg_item = item("CFG-0001");
        cfg_item.project = "config-handler".to_string();
        let mut pwf_item = item("PWF-9999");
        pwf_item.project = "pwf".to_string();
        let mut cfg_newer_item = item("CFG-0002");
        cfg_newer_item.project = "config-handler".to_string();

        let order = OrderSpec {
            field: OrderField::ProjectId,
            direction: OrderDirection::Asc,
        };
        let mut items = vec![pwf_item, cfg_item, cfg_newer_item];
        sort_by_order(&mut items, order);

        // project-name ascending ("config-handler" < "pwf"), then newest-id-first
        // within each project.
        assert_eq!(ids(&items), ["CFG-0002", "CFG-0001", "PWF-9999"]);
    }

    #[test]
    fn sort_by_order_project_id_desc_reverses_only_the_project_axis() {
        let mut cfg_item = item("CFG-0001");
        cfg_item.project = "config-handler".to_string();
        let mut pwf_item = item("PWF-9999");
        pwf_item.project = "pwf".to_string();

        let order = OrderSpec {
            field: OrderField::ProjectId,
            direction: OrderDirection::Desc,
        };
        let mut items = vec![cfg_item, pwf_item];
        sort_by_order(&mut items, order);
        // "pwf" > "config-handler", so desc puts pwf's project first; the id
        // sub-key stays fixed newest-first regardless of direction.
        assert_eq!(ids(&items), ["PWF-9999", "CFG-0001"]);
    }

    #[test]
    fn sort_by_order_default_orders_newest_created_first_across_projects() {
        let mut cfg_item = dated("CFG-0001", "2026-03-01");
        cfg_item.project = "config-handler".to_string();
        let mut pwf_item = dated("PWF-0001", "2026-01-01");
        pwf_item.project = "pwf".to_string();

        // "config-handler" < "pwf" alphabetically, but CFG-0001 was created
        // later, so the (project-flat) created-desc default must still put it
        // first — this is the exact bug report: date must win over project.
        let mut items = vec![pwf_item, cfg_item];
        sort_by_order(&mut items, OrderSpec::default());
        assert_eq!(ids(&items), ["CFG-0001", "PWF-0001"]);
    }

    #[test]
    fn sort_by_order_created_asc_orders_oldest_first() {
        let order = OrderSpec {
            field: OrderField::Created,
            direction: OrderDirection::Asc,
        };
        let mut items = vec![
            dated("GLP-0001", "2026-01-01"),
            dated("GLP-0002", "2026-03-01"),
            dated("GLP-0003", "2026-02-01"),
        ];
        sort_by_order(&mut items, order);
        assert_eq!(ids(&items), ["GLP-0001", "GLP-0003", "GLP-0002"]);
    }

    #[test]
    fn sort_by_order_created_ties_tiebreak_by_id() {
        // Items sharing a `created:` value (e.g. same-day adds) still sort
        // deterministically, by full id, direction-consistent with the primary key.
        let mut items = vec![
            dated("GLP-0002", "2026-01-01"),
            dated("GLP-0001", "2026-01-01"),
        ];
        sort_by_order(&mut items, OrderSpec::default());
        assert_eq!(ids(&items), ["GLP-0002", "GLP-0001"]);
    }

    #[test]
    fn from_tokens_project_id_alone_defaults_direction_to_asc() {
        let spec = OrderSpec::from_tokens(&["project-id".to_string()]).unwrap();
        assert_eq!(spec.field, OrderField::ProjectId);
        assert_eq!(spec.direction, OrderDirection::Asc);
    }

    #[test]
    fn apply_cap_default_keeps_first_n_after_sort() {
        let items: Vec<Item> = (1..=12).map(|n| item(&format!("GLP-{n:04}"))).collect();
        let (kept, hidden) = apply_cap(items, DEFAULT_LIST_CAP);
        assert_eq!(kept.len(), 10);
        assert_eq!(hidden, 2);
    }

    #[test]
    fn apply_cap_zero_is_unlimited() {
        let items: Vec<Item> = (1..=12).map(|n| item(&format!("GLP-{n:04}"))).collect();
        let (kept, hidden) = apply_cap(items, 0);
        assert_eq!(kept.len(), 12);
        assert_eq!(hidden, 0);
    }

    #[test]
    fn apply_cap_larger_than_len_keeps_all() {
        let items = vec![item("GLP-0001"), item("GLP-0002"), item("GLP-0003")];
        let (kept, hidden) = apply_cap(items, 10);
        assert_eq!(kept.len(), 3);
        assert_eq!(hidden, 0);
    }

    #[test]
    fn effort_filter_keeps_only_matching_tier() {
        let mut low = item("GLP-0001");
        low.effort = Some("1".to_string());
        let mut high = item("GLP-0002");
        high.effort = Some("4".to_string());
        let untagged = item("GLP-0003");

        let items = vec![low, high, untagged];
        let filtered: Vec<_> = items
            .into_iter()
            .filter(|i| effort_matches(i, Some(1)))
            .collect();

        assert_eq!(ids(&filtered), ["GLP-0001"]);
    }

    #[test]
    fn effort_filter_none_keeps_everything() {
        let mut tagged = item("GLP-0001");
        tagged.effort = Some("2".to_string());
        let untagged = item("GLP-0002");

        let items = vec![tagged, untagged];
        let filtered: Vec<_> = items
            .into_iter()
            .filter(|i| effort_matches(i, None))
            .collect();

        assert_eq!(ids(&filtered), ["GLP-0001", "GLP-0002"]);
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
