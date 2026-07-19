use std::sync::LazyLock;

use pwf_domain::pending_work::{ParsePrereqsError, Prereqs, WorkItemStatus};
use regex::Regex;

use super::{errors::PendingWorkError, naming::project_dir};
use crate::config::Config;

const PREREQ_VALUE_PATTERN: &str = r"\[\[([A-Z]{2,4}-\d{4})";
static PREREQ_VALUE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(PREREQ_VALUE_PATTERN).unwrap());

/// Contains prerequisite IDs confirmed to exist in the current configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct KnownPrereqs(Prereqs);

impl KnownPrereqs {
    pub(super) fn from_flags(
        cfg: &Config,
        values: &[String],
    ) -> Result<Option<Self>, PendingWorkError> {
        if values.is_empty() {
            return Ok(None);
        }
        let prereqs = parse_flag_values(values)?;
        let ids = prereqs.ids();
        let missing: Vec<_> = ids
            .iter()
            .filter(|id| read_status(cfg, id).is_none())
            .map(|id| (*id).to_string())
            .collect();
        if !missing.is_empty() {
            return Err(PendingWorkError::UnknownPrereqIds { ids: missing });
        }
        Ok(Some(Self(prereqs)))
    }

    pub(super) fn frontmatter_value(&self) -> String {
        self.0.frontmatter_value()
    }
}

pub(super) fn frontmatter_from_flags(
    cfg: &Config,
    values: &[String],
) -> Result<Option<String>, PendingWorkError> {
    Ok(KnownPrereqs::from_flags(cfg, values)?.map(|p| p.frontmatter_value()))
}

fn parse_flag_values(values: &[String]) -> Result<Prereqs, PendingWorkError> {
    Prereqs::parse_values(values).map_err(map_parse_prereqs_error)
}

#[cfg(test)]
fn parse_flag_ids(values: &[String]) -> Result<Vec<String>, PendingWorkError> {
    Ok(parse_flag_values(values)?
        .ids()
        .into_iter()
        .map(str::to_string)
        .collect())
}

fn map_parse_prereqs_error(error: ParsePrereqsError) -> PendingWorkError {
    match error {
        ParsePrereqsError::MissingId => PendingWorkError::MissingPrereqId,
        ParsePrereqsError::InvalidId { raw } => PendingWorkError::InvalidPrereqId { raw },
    }
}

pub(super) struct PrereqStatus {
    pub id: String,
    pub status: Option<WorkItemStatus>,
}

impl PrereqStatus {
    fn label(&self) -> String {
        self.status
            .map_or_else(|| "missing".to_string(), |status| status.to_string())
    }
}

/// Resolves each prerequisite wikilink to its work-item status.
pub(super) fn resolve(cfg: &Config, prereq: &str) -> Vec<PrereqStatus> {
    PREREQ_VALUE_RE
        .captures_iter(prereq)
        .map(|c| {
            let id = c[1].to_string();
            let status = read_status(cfg, &id);
            PrereqStatus { id, status }
        })
        .collect()
}

/// Reads status after mapping the ID prefix to a managed project.
/// Returns `None` when the project, file, or status is missing.
fn read_status(cfg: &Config, id: &str) -> Option<WorkItemStatus> {
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
        .and_then(|status| status.parse().ok())
}

/// Formats prerequisite statuses for the long list view.
pub(super) fn list_summary(statuses: &[PrereqStatus]) -> String {
    statuses
        .iter()
        .map(|s| format!("{} ({})", s.id, s.label()))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, fs};

    use super::*;
    use crate::engines::pending_work::errors::PendingWorkError;

    fn stage_cfg() -> (tempfile::TempDir, Config) {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("config-handler");
        fs::create_dir_all(&proj).unwrap();
        fs::write(proj.join("CFG-0014.md"), "---\nstatus: done\n---\nbody\n").unwrap();
        fs::write(proj.join("CFG-0015.md"), "---\nstatus: active\n---\nbody\n").unwrap();
        let json = format!(
            r#"{{ "notesDir": "{}", "projects": {{ "config-handler": "/r" }}, "prefixes": {{ "config-handler": "CFG" }} }}"#,
            dir.path().to_string_lossy().replace('\\', "\\\\")
        );
        let cfg = crate::config::from_json(&json, None).unwrap();
        (dir, cfg)
    }

    #[test]
    fn resolve_reads_done_active_and_missing() {
        let (_d, cfg) = stage_cfg();
        let got = resolve(&cfg, "[[CFG-0014]] [[CFG-0015]] [[CFG-9999]]");
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].status, Some(WorkItemStatus::Done));
        assert_eq!(got[0].label(), "done");
        assert_eq!(got[1].status, Some(WorkItemStatus::Active));
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
        let prereqs = KnownPrereqs::from_flags(&cfg, &values).unwrap().unwrap();
        assert_eq!(prereqs.frontmatter_value(), "[[CFG-0014]], [[CFG-0015]]");
    }

    #[test]
    fn prereqs_reject_malformed_ids_with_typed_error_and_legacy_display() {
        let err = parse_flag_ids(&["CFG-99999".to_string()]).unwrap_err();
        assert_matches!(
            err,
            PendingWorkError::InvalidPrereqId { ref raw } if raw == "CFG-99999"
        );
        assert_eq!(err.to_string(), "Invalid --prereq id: CFG-99999.");
    }

    #[test]
    fn prereqs_normalize_lowercase_ids() {
        let ids = parse_flag_ids(&["cfg-0014".to_string()]).unwrap();
        assert_eq!(ids, vec!["CFG-0014".to_string()]);
    }

    #[test]
    fn prereqs_normalize_shorthand_ids() {
        // Uses the same shorthand canonicalization as `--id`.
        let ids = parse_flag_ids(&[
            "cfg57".to_string(),
            "CFG-14".to_string(),
            "cfg-0015".to_string(),
        ])
        .unwrap();
        assert_eq!(
            ids,
            vec![
                "CFG-0057".to_string(),
                "CFG-0014".to_string(),
                "CFG-0015".to_string()
            ]
        );
    }

    #[test]
    fn prereqs_reject_empty_values_with_typed_error_and_legacy_display() {
        let err = parse_flag_ids(&[]).unwrap_err();

        assert_matches!(err, PendingWorkError::MissingPrereqId);
        assert_eq!(err.to_string(), "--prereq requires an id.");
    }

    #[test]
    fn prereqs_reject_blank_or_comma_only_values_as_missing_ids() {
        for values in [
            vec![String::new()],
            vec![", ,".to_string()],
            vec![" ".to_string(), ",".to_string()],
        ] {
            let err = parse_flag_ids(&values).unwrap_err();

            assert_matches!(err, PendingWorkError::MissingPrereqId);
            assert_eq!(err.to_string(), "--prereq requires an id.");
        }
    }

    #[test]
    fn prereqs_reject_missing_ids() {
        let (_d, cfg) = stage_cfg();
        let err = KnownPrereqs::from_flags(&cfg, &["CFG-9999".to_string()]).unwrap_err();
        assert_matches!(
            err,
            PendingWorkError::UnknownPrereqIds { ref ids }
                if ids == &vec!["CFG-9999".to_string()]
        );
        assert_eq!(err.to_string(), "Unknown --prereq id(s): CFG-9999.");
    }
}
