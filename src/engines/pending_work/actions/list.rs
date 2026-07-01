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

/// Reorder items by project, then newest-first inside each project. The full id
/// string tiebreaks so the order is deterministic across runs.
fn sort_by_project_then_newest(items: &mut [Item]) {
    items.sort_by(|a, b| {
        a.project
            .cmp(&b.project)
            .then_with(|| id_suffix(&b.id).cmp(&id_suffix(&a.id)))
            .then_with(|| b.id.cmp(&a.id))
    });
}

fn sort_by_group_then_project_then_newest(items: &mut [Item]) {
    items.sort_by(|a, b| {
        section_group_rank(a.section.as_deref())
            .cmp(&section_group_rank(b.section.as_deref()))
            .then_with(|| a.project.cmp(&b.project))
            .then_with(|| id_suffix(&b.id).cmp(&id_suffix(&a.id)))
            .then_with(|| b.id.cmp(&a.id))
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
) -> Result<String, PendingWorkError> {
    let mut items: Vec<_> = get_pending_work(cfg, only_project)?
        .into_iter()
        .filter(|i| scope.includes(i.section.as_deref()))
        .filter(|i| effort_matches(i, effort))
        .collect();
    // Order + cap before rendering so the selected/capped sequence is consistent.
    if scope.groups_output() {
        sort_by_group_then_project_then_newest(&mut items);
    } else {
        sort_by_project_then_newest(&mut items);
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

        let err = run_list_action(&cfg, None, false, ListScope::Default, None, None).unwrap_err();

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
    fn sort_by_project_then_newest_orders_one_project_by_descending_id() {
        let mut items = vec![item("GLP-0001"), item("GLP-0003"), item("GLP-0002")];
        sort_by_project_then_newest(&mut items);
        assert_eq!(ids(&items), ["GLP-0003", "GLP-0002", "GLP-0001"]);
    }

    #[test]
    fn sort_by_project_then_newest_groups_by_project_before_descending_id() {
        let mut cfg_item = item("CFG-0001");
        cfg_item.project = "config-handler".to_string();
        let mut pwf_item = item("PWF-9999");
        pwf_item.project = "pwf".to_string();
        let mut cfg_newer_item = item("CFG-0002");
        cfg_newer_item.project = "config-handler".to_string();

        let mut items = vec![pwf_item, cfg_item, cfg_newer_item];
        sort_by_project_then_newest(&mut items);

        assert_eq!(ids(&items), ["CFG-0002", "CFG-0001", "PWF-9999"]);
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
}
