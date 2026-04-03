// Resolve `prereq` frontmatter wikilinks to their work-item status, so launch can
// warn when a prerequisite item is not done yet.

use super::model::Item;
use super::naming::project_dir;
use crate::config::Config;
use regex::Regex;

const PREREQ_ID_PATTERN: &str = r"^(?:\[\[)?(?P<id>[A-Z]{2,4}-\d{4})(?:\]\])?$";

// Regex reading bare ids out of an existing `prereq` frontmatter value.
const PREREQ_VALUE_PATTERN: &str = r"\[\[([A-Z]{2,4}-\d{4})";

/// Renders `ids` as `[[X]], [[Y]]`, the canonical `prereq` frontmatter value.
fn render_ids(ids: &[String]) -> String {
    ids.iter()
        .map(|id| format!("[[{id}]]"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Prereqs {
    ids: Vec<String>,
}

impl Prereqs {
    pub(super) fn from_flags(cfg: &Config, values: &[String]) -> Result<Option<Self>, String> {
        if values.is_empty() {
            return Ok(None);
        }
        let ids = parse_flag_ids(values)?;
        let missing: Vec<_> = ids
            .iter()
            .filter(|id| read_status(cfg, id).is_none())
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(format!("Unknown --prereq id(s): {}.", missing.join(", ")));
        }
        Ok(Some(Self { ids }))
    }

    pub(super) fn frontmatter_value(&self) -> String {
        render_ids(&self.ids)
    }
}

/// Appends validated `values` to an existing `prereq` frontmatter value, deduplicating.
///
/// `existing` is a frontmatter value such as `"[[CFG-0014]]"` (or `None` when the item
/// has no prereq). Returns the merged `[[X]], [[Y]]` value with existing ids first,
/// then the new ones, each id kept only on first appearance.
///
/// # Errors
/// Errors identically to [`Prereqs::from_flags`] when any new id is unknown.
pub(super) fn append_to_frontmatter(
    cfg: &Config,
    existing: Option<&str>,
    values: &[String],
) -> Result<String, String> {
    let value_re = Regex::new(PREREQ_VALUE_PATTERN).unwrap();
    let mut ids: Vec<String> = existing
        .into_iter()
        .flat_map(|v| value_re.captures_iter(v).map(|c| c[1].to_string()))
        .collect();
    let new = Prereqs::from_flags(cfg, values)?
        .map(|p| p.ids)
        .unwrap_or_default();
    for id in new {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(render_ids(&ids))
}

pub(super) fn frontmatter_from_flags(
    cfg: &Config,
    values: &[String],
) -> Result<Option<String>, String> {
    Ok(Prereqs::from_flags(cfg, values)?.map(|p| p.frontmatter_value()))
}

fn parse_flag_ids(values: &[String]) -> Result<Vec<String>, String> {
    let id_re = Regex::new(PREREQ_ID_PATTERN).unwrap();
    let mut ids = Vec::new();
    for value in values {
        for raw in value.split(',') {
            let raw = raw.trim();
            let caps = id_re
                .captures(raw)
                .ok_or_else(|| format!("Invalid --prereq id: {raw}."))?;
            let id = caps["id"].to_string();
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    if ids.is_empty() {
        return Err("--prereq requires an id.".to_string());
    }
    Ok(ids)
}

/// One resolved prerequisite: its id and the backing note's `status` (None when the
/// note is missing).
pub(super) struct PrereqStatus {
    pub id: String,
    pub status: Option<String>,
}

impl PrereqStatus {
    pub(super) fn is_done(&self) -> bool {
        self.status.as_deref() == Some("done")
    }

    /// Human label: the raw status, or "missing" when the note is absent.
    fn label(&self) -> &str {
        self.status.as_deref().unwrap_or("missing")
    }
}

/// Resolves every `[[AAA-NNNN]]` wikilink in `prereq` to its status.
pub(super) fn resolve(cfg: &Config, prereq: &str) -> Vec<PrereqStatus> {
    let id_re = Regex::new(PREREQ_VALUE_PATTERN).unwrap();
    id_re
        .captures_iter(prereq)
        .map(|c| {
            let id = c[1].to_string();
            let status = read_status(cfg, &id);
            PrereqStatus { id, status }
        })
        .collect()
}

/// Reads the `status` frontmatter of the note backing `id`, mapping its prefix to a
/// managed project. Returns None when the project, file, or status is absent.
fn read_status(cfg: &Config, id: &str) -> Option<String> {
    let prefix = id.split('-').next().unwrap_or("");
    let project = cfg
        .prefixes
        .iter()
        .find(|(_, p)| p.as_str() == prefix)
        .map(|(proj, _)| proj.as_str())?;
    let path = project_dir(cfg.notes_dir_for(project), project).join(format!("{id}.md"));
    let raw = std::fs::read_to_string(path).ok()?;
    crate::frontmatter::parse(&raw)
        .frontmatter
        .get("status")
        .cloned()
}

/// Renders `CFG-0014 (done), CFG-0015 (active)` for the `--long` list line.
pub(super) fn list_summary(statuses: &[PrereqStatus]) -> String {
    statuses
        .iter()
        .map(|s| format!("{} ({})", s.id, s.label()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A stderr warning naming each unsatisfied (non-done) prerequisite, or None when all
/// are done (or there are none).
pub(super) fn launch_warning(statuses: &[PrereqStatus]) -> Option<String> {
    let lines: Vec<String> = statuses
        .iter()
        .filter(|s| !s.is_done())
        .map(|s| format!("WARN: prereq {} is not done ({}).", s.id, s.label()))
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Resolves an item's `prereq` (if any) and prints a stderr warning for each
/// unsatisfied prerequisite. Launch proceeds regardless (warn-only).
pub(super) fn warn_unsatisfied_on_launch(cfg: &Config, item: &Item) {
    if let Some(pq) = &item.prereq
        && let Some(w) = launch_warning(&resolve(cfg, pq))
    {
        eprintln!("{w}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    fn stage_cfg() -> (std::path::PathBuf, Config) {
        let dir = std::env::temp_dir().join(format!("pwprereq_{}", nanos()));
        let proj = dir.join("config-handler");
        fs::create_dir_all(&proj).unwrap();
        fs::write(proj.join("CFG-0014.md"), "---\nstatus: done\n---\nbody\n").unwrap();
        fs::write(proj.join("CFG-0015.md"), "---\nstatus: active\n---\nbody\n").unwrap();
        let json = format!(
            r#"{{ "notesDir": "{}", "projects": {{ "config-handler": "/r" }}, "prefixes": {{ "config-handler": "CFG" }} }}"#,
            dir.to_string_lossy().replace('\\', "\\\\")
        );
        let cfg = crate::config::from_json(&json, None).unwrap();
        (dir, cfg)
    }

    #[test]
    fn resolve_reads_done_active_and_missing() {
        let (_d, cfg) = stage_cfg();
        let got = resolve(&cfg, "[[CFG-0014]] [[CFG-0015]] [[CFG-9999]]");
        assert_eq!(got.len(), 3);
        assert!(got[0].is_done());
        assert_eq!(got[0].label(), "done");
        assert!(!got[1].is_done());
        assert_eq!(got[1].label(), "active");
        assert!(!got[2].is_done());
        assert_eq!(got[2].label(), "missing");
    }

    #[test]
    fn list_summary_joins_id_and_status() {
        let (_d, cfg) = stage_cfg();
        let got = resolve(&cfg, "[[CFG-0014]] [[CFG-0015]]");
        assert_eq!(list_summary(&got), "CFG-0014 (done), CFG-0015 (active)");
    }

    #[test]
    fn launch_warning_only_for_unsatisfied() {
        let (_d, cfg) = stage_cfg();
        assert!(launch_warning(&resolve(&cfg, "[[CFG-0014]]")).is_none());
        let w = launch_warning(&resolve(&cfg, "[[CFG-0015]]")).unwrap();
        assert!(
            w.contains("WARN: prereq CFG-0015 is not done (active)."),
            "got: {w}"
        );
    }

    #[test]
    fn prereqs_accept_repeatable_and_comma_values() {
        let (_d, cfg) = stage_cfg();
        let values = vec!["CFG-0014, CFG-0015".to_string(), "[[CFG-0014]]".to_string()];
        let prereqs = Prereqs::from_flags(&cfg, &values).unwrap().unwrap();
        assert_eq!(prereqs.frontmatter_value(), "[[CFG-0014]], [[CFG-0015]]");
    }

    #[test]
    fn prereqs_reject_missing_ids() {
        let (_d, cfg) = stage_cfg();
        let err = Prereqs::from_flags(&cfg, &["CFG-9999".to_string()]).unwrap_err();
        assert!(err.contains("CFG-9999"), "got: {err}");
    }

    #[test]
    fn append_to_empty_sets_value() {
        let (_d, cfg) = stage_cfg();
        let got = append_to_frontmatter(&cfg, None, &["CFG-0014".to_string()]).unwrap();
        assert_eq!(got, "[[CFG-0014]]");
    }

    #[test]
    fn append_dedups_against_existing() {
        let (_d, cfg) = stage_cfg();
        let got = append_to_frontmatter(
            &cfg,
            Some("[[CFG-0014]]"),
            &["CFG-0014".to_string(), "CFG-0015".to_string()],
        )
        .unwrap();
        assert_eq!(got, "[[CFG-0014]], [[CFG-0015]]");
    }

    #[test]
    fn append_rejects_unknown() {
        let (_d, cfg) = stage_cfg();
        let err = append_to_frontmatter(&cfg, None, &["CFG-9999".to_string()]).unwrap_err();
        assert!(err.contains("CFG-9999"), "got: {err}");
    }
}
