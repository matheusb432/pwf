// Resolve `prereq` frontmatter wikilinks to their work-item status for list and
// other consumers that need to know whether prerequisite items are complete.

use regex::Regex;

use super::{domain::types::WorkItemId, errors::PendingWorkError, naming::project_dir};
use crate::config::Config;

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
    pub(super) fn from_flags(
        cfg: &Config,
        values: &[String],
    ) -> Result<Option<Self>, PendingWorkError> {
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
            return Err(PendingWorkError::UnknownPrereqIds { ids: missing });
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
) -> Result<String, PendingWorkError> {
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
) -> Result<Option<String>, PendingWorkError> {
    Ok(Prereqs::from_flags(cfg, values)?.map(|p| p.frontmatter_value()))
}

fn parse_flag_ids(values: &[String]) -> Result<Vec<String>, PendingWorkError> {
    let mut ids = Vec::new();
    for value in values {
        for raw in value.split(',') {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            let id = raw
                .strip_prefix("[[")
                .and_then(|id| id.strip_suffix("]]"))
                .unwrap_or(raw);
            let id = WorkItemId::try_new(id)
                .map_err(|_| PendingWorkError::InvalidPrereqId {
                    raw: raw.to_string(),
                })?
                .to_string();
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    if ids.is_empty() {
        return Err(PendingWorkError::MissingPrereqId);
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::engines::pending_work::errors::PendingWorkError;

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
        assert_eq!(got[0].status.as_deref(), Some("done"));
        assert_eq!(got[0].label(), "done");
        assert_eq!(got[1].status.as_deref(), Some("active"));
        assert_eq!(got[1].label(), "active");
        assert_eq!(got[2].status, None);
        assert_eq!(got[2].label(), "missing");
    }

    #[test]
    fn list_summary_joins_id_and_status() {
        let (_d, cfg) = stage_cfg();
        let got = resolve(&cfg, "[[CFG-0014]] [[CFG-0015]]");
        assert_eq!(list_summary(&got), "CFG-0014 (done), CFG-0015 (active)");
    }

    #[test]
    fn prereqs_accept_repeatable_and_comma_values() {
        let (_d, cfg) = stage_cfg();
        let values = vec!["CFG-0014, CFG-0015".to_string(), "[[CFG-0014]]".to_string()];
        let prereqs = Prereqs::from_flags(&cfg, &values).unwrap().unwrap();
        assert_eq!(prereqs.frontmatter_value(), "[[CFG-0014]], [[CFG-0015]]");
    }

    #[test]
    fn prereqs_reject_malformed_ids_with_typed_error_and_legacy_display() {
        let err = parse_flag_ids(&["CFG-12".to_string()]).unwrap_err();
        assert!(matches!(
            err,
            PendingWorkError::InvalidPrereqId { ref raw } if raw == "CFG-12"
        ));
        assert_eq!(err.to_string(), "Invalid --prereq id: CFG-12.");
    }

    #[test]
    fn prereqs_normalize_lowercase_ids() {
        // lowercase is accepted and canonicalized (PWF-0038 / PWF-FR-012).
        let ids = parse_flag_ids(&["cfg-0014".to_string()]).unwrap();
        assert_eq!(ids, vec!["CFG-0014".to_string()]);
    }

    #[test]
    fn prereqs_reject_empty_values_with_typed_error_and_legacy_display() {
        let err = parse_flag_ids(&[]).unwrap_err();

        assert!(matches!(err, PendingWorkError::MissingPrereqId));
        assert_eq!(err.to_string(), "--prereq requires an id.");
    }

    #[test]
    fn prereqs_reject_blank_or_comma_only_values_as_missing_ids() {
        for values in [
            vec!["".to_string()],
            vec![", ,".to_string()],
            vec![" ".to_string(), ",".to_string()],
        ] {
            let err = parse_flag_ids(&values).unwrap_err();

            assert!(matches!(err, PendingWorkError::MissingPrereqId));
            assert_eq!(err.to_string(), "--prereq requires an id.");
        }
    }

    #[test]
    fn prereqs_reject_missing_ids() {
        let (_d, cfg) = stage_cfg();
        let err = Prereqs::from_flags(&cfg, &["CFG-9999".to_string()]).unwrap_err();
        assert!(matches!(
            err,
            PendingWorkError::UnknownPrereqIds { ref ids }
                if ids == &vec!["CFG-9999".to_string()]
        ));
        assert_eq!(err.to_string(), "Unknown --prereq id(s): CFG-9999.");
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
        assert!(matches!(
            err,
            PendingWorkError::UnknownPrereqIds { ref ids }
                if ids == &vec!["CFG-9999".to_string()]
        ));
        assert_eq!(err.to_string(), "Unknown --prereq id(s): CFG-9999.");
    }
}
