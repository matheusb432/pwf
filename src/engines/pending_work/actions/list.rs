// Action: list.

use super::super::domain::read_models::ListResult;
use super::super::errors::PendingWorkError;
use super::super::model::Item;
use super::super::query::get_pending_work;
use super::super::render::render_list;
use crate::config::Config;

/// Default item cap for `pw list` when `-n` is absent; keeps agents from being
/// flooded with tokens (PWF-0020). `-n 0` overrides to unlimited.
const DEFAULT_LIST_CAP: usize = 10;

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

/// List action implementation. `## Future` and `## Human` items are hidden unless
/// `show_future` / `show_human` re-include them.
pub(in crate::engines::pending_work) fn run_list_action(
    cfg: &Config,
    only_project: Option<&str>,
    json: bool,
    long: bool,
    show_future: bool,
    show_human: bool,
    number: Option<usize>,
) -> Result<String, PendingWorkError> {
    let mut items: Vec<_> = get_pending_work(cfg, only_project)?
        .into_iter()
        .filter(|i| match i.section.as_deref() {
            Some("Future") => show_future,
            Some("Human") => show_human,
            _ => true,
        })
        .collect();
    // Order + cap before the JSON branch so `--json` follows the same selection.
    sort_by_project_then_newest(&mut items);
    let (items, hidden) = apply_cap(items, number.unwrap_or(DEFAULT_LIST_CAP));
    let result = ListResult::from_items(items, hidden);
    Ok(render_list(&result, cfg, only_project, json, long))
}

#[cfg(test)]
mod tests {
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

        let err = run_list_action(&cfg, None, false, false, false, false, None).unwrap_err();

        assert!(matches!(
            err,
            PendingWorkError::NotesDirectoryNotFound { ref path }
                if path == &missing
        ));
        assert_eq!(err.to_string(), format!("Notes directory not found: {missing}"));
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
}
