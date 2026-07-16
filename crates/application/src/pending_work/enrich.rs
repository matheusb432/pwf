//! Launchability enrichment for open pending-work items.
//!
//! The pure derivation of an [`OpenItem`]'s launch-diagnostic fields
//! (`launchable`/`needs_prompt`/`issues`) plus the display `format`/`section`
//! normalization — relocated here from the infra read parser (PWF-0123 Task
//! 2.4, since retired) so the generic-port read handlers own one copy of the
//! rules. Every rule is transliterated verbatim; changing
//! any of them changes `pwf list` diagnostics and `verify`/`session`
//! launchability.

use std::sync::LazyLock;

use pwf_domain::pending_work::OpenItem;
use regex::Regex;

use crate::{Materialization, PendingWorkItem, RecordId};

/// Diagnostic emitted when a project note has no configured repo mapping.
pub const ISSUE_NO_REPO: &str =
    "Project note is not mapped to a repo; add it to config/pending-work.json.";
/// Diagnostic emitted when a prompt is still a placeholder.
pub const ISSUE_PLACEHOLDER_PROMPT: &str =
    "Prompt is a placeholder; define a real prompt before launching.";

static PLACEHOLDER_PROMPT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(^\s*\[!\]\s*TODO\b|^\s*TODO\b|definir prompt|define prompt|tbd)")
        .expect("valid placeholder regex")
});

/// Whether a prompt is empty or matches a known placeholder marker.
#[must_use]
pub fn is_placeholder_prompt(prompt: &str) -> bool {
    prompt.trim().is_empty() || PLACEHOLDER_PROMPT_RE.is_match(prompt)
}

/// Canonicalizes a raw index section label to its display form; unknown labels
/// pass through trimmed. Application owns this normalization (the infra
/// `IndexEntry`/record `section` stay raw).
#[must_use]
pub fn normalize_section_label(label: &str) -> String {
    match label.trim().to_lowercase().as_str() {
        "future" | "futuro" => "Future".to_string(),
        "human" => "Human".to_string(),
        "low-prio" | "low-priority" => "Low-prio".to_string(),
        _ => label.trim().to_string(),
    }
}

/// Normalizes an optional raw section label, preserving `None`.
#[must_use]
pub fn normalize_section(section: Option<&str>) -> Option<String> {
    section.map(normalize_section_label)
}

/// The derived launch-diagnostic flags for an open item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedFlags {
    pub issues: Vec<String>,
    pub launchable: bool,
    pub needs_prompt: bool,
}

/// Computes `issues`/`launchable`/`needs_prompt` from a repo mapping, the
/// resolved prompt, and an optional missing-note locator — in the exact order
/// the legacy parser emitted them (no-repo, missing-note, placeholder).
#[must_use]
pub fn derive_flags(repo: Option<&str>, prompt: &str, missing_note: Option<&str>) -> DerivedFlags {
    let mut issues = Vec::new();
    if repo.is_none_or(|value| value.trim().is_empty()) {
        issues.push(ISSUE_NO_REPO.to_string());
    }
    if let Some(path) = missing_note {
        issues.push(format!("Work-item note missing: {path}"));
    }
    let needs_prompt = is_placeholder_prompt(prompt);
    if needs_prompt {
        issues.push(ISSUE_PLACEHOLDER_PROMPT.to_string());
    }
    DerivedFlags {
        launchable: issues.is_empty(),
        needs_prompt,
        issues,
    }
}

/// An open pending-work item enriched with its launch diagnostics, minus the
/// owning `project` (supplied by the read handler via [`Self::into_open_item`],
/// which also composes a legacy inline record's `<project>:<ordinal>` id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrichedOpenItem {
    pub id: RecordId,
    pub session: String,
    pub prompt: String,
    pub repo: Option<String>,
    pub note: String,
    pub item_file: Option<String>,
    pub line: usize,
    pub format: String,
    pub launchable: bool,
    pub needs_prompt: bool,
    pub issues: Vec<String>,
    pub section: Option<String>,
    pub prereq: Option<String>,
    pub effort: Option<String>,
    pub tags: Option<String>,
    pub created: Option<String>,
}

impl EnrichedOpenItem {
    /// Attaches the owning project, yielding the domain [`OpenItem`].
    #[must_use]
    pub fn into_open_item(self, project: String) -> OpenItem {
        let id = match &self.id {
            RecordId::Item(id) => id.as_ref().to_string(),
            RecordId::Inline(ordinal) => format!("{project}:{ordinal}"),
        };
        OpenItem {
            id,
            project,
            session: self.session,
            prompt: self.prompt,
            repo: self.repo,
            note: self.note,
            item_file: self.item_file,
            line: self.line,
            format: self.format,
            launchable: self.launchable,
            needs_prompt: self.needs_prompt,
            issues: self.issues,
            section: self.section,
            prereq: self.prereq,
            effort: self.effort,
            tags: self.tags,
            created: self.created,
        }
    }
}

/// Enriches a stored [`PendingWorkItem`] into an open-item projection,
/// reproducing the legacy read's derived fields exactly:
///
/// - `format`/`item_file` come off the materialization (`file` for note-backed and missing-note
///   wikilinks, `legacy` + no file for inline prompts);
/// - a missing note contributes its `Work-item note missing: <path>` issue;
/// - `note`/`line` render the record's index placement;
/// - an empty title falls back to the canonical id;
/// - the raw section label is canonicalized.
#[must_use]
pub fn enrich(item: &PendingWorkItem, repo: Option<&str>) -> EnrichedOpenItem {
    let prompt = item.body.trim().to_string();
    let (format, item_file, missing_note) = match &item.materialization {
        Materialization::NoteFile => ("file", Some(item.locator.clone()), None),
        Materialization::MissingNote { expected } => {
            ("file", Some(item.locator.clone()), Some(expected.as_str()))
        }
        Materialization::InlineLegacy => ("legacy", None, None),
    };
    let flags = derive_flags(repo, &prompt, missing_note);
    let session = match &item.id {
        RecordId::Item(id) if item.title.trim().is_empty() => id.as_ref().to_string(),
        _ => item.title.clone(),
    };
    let (note, line) = item.placement.as_ref().map_or_else(
        || (item.locator.clone(), 1),
        |placement| (placement.index_path.clone(), placement.line),
    );
    EnrichedOpenItem {
        id: item.id.clone(),
        session,
        prompt,
        repo: repo.map(str::to_string),
        note,
        line,
        item_file,
        format: format.to_string(),
        launchable: flags.launchable,
        needs_prompt: flags.needs_prompt,
        issues: flags.issues,
        section: normalize_section(item.section.as_deref()),
        prereq: item.prereq.clone(),
        effort: item.effort.clone(),
        tags: item.tags.clone(),
        created: item.created.as_ref().map(|ts| ts.as_str().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use pwf_domain::pending_work::{Timestamp, WorkItemId, WorkItemStatus};

    use super::*;
    use crate::IndexPlacement;

    fn record(body: &str) -> PendingWorkItem {
        PendingWorkItem {
            id: RecordId::Item(WorkItemId::try_new("PWF-0001").unwrap()),
            title: "tray gui".to_string(),
            status: WorkItemStatus::Active,
            created: Some(Timestamp::new("2026-01-01".to_string())),
            completed: None,
            commits: None,
            tags: None,
            effort: None,
            prereq: None,
            section: None,
            body: body.to_string(),
            source: String::new(),
            locator: "/notes/pwf/PWF-0001.md".to_string(),
            placement: Some(IndexPlacement {
                index_path: "/notes/pwf/pwf.md".to_string(),
                line: 7,
            }),
            materialization: Materialization::NoteFile,
        }
    }

    #[test]
    fn launchable_when_repo_present_and_prompt_real() {
        let enriched = enrich(&record("add startup toggle"), Some("/repo"));
        assert!(enriched.launchable);
        assert!(!enriched.needs_prompt);
        assert!(enriched.issues.is_empty());
        assert_eq!(enriched.prompt, "add startup toggle");
        assert_eq!(enriched.repo.as_deref(), Some("/repo"));
    }

    #[test]
    fn note_and_line_render_the_index_placement_not_the_note_file() {
        let enriched = enrich(&record("body"), Some("/repo"));
        assert_eq!(enriched.note, "/notes/pwf/pwf.md");
        assert_eq!(enriched.line, 7);
        assert_eq!(
            enriched.item_file.as_deref(),
            Some("/notes/pwf/PWF-0001.md")
        );
        assert_eq!(enriched.format, "file");
    }

    #[test]
    fn missing_repo_and_placeholder_prompt_are_not_launchable() {
        let enriched = enrich(&record("TODO"), None);
        assert!(!enriched.launchable);
        assert!(enriched.needs_prompt);
        assert_eq!(
            enriched.issues,
            [
                ISSUE_NO_REPO.to_string(),
                ISSUE_PLACEHOLDER_PROMPT.to_string()
            ]
        );
    }

    #[test]
    fn missing_note_wikilink_needs_attention_with_missing_note_issue() {
        let mut rec = record("");
        rec.materialization = Materialization::MissingNote {
            expected: "/notes/pwf/PWF-0001.md".to_string(),
        };

        let enriched = enrich(&rec, Some("/repo"));

        assert!(!enriched.launchable, "missing note must not be launchable");
        assert!(enriched.needs_prompt, "empty prompt is a placeholder");
        assert_eq!(
            enriched.issues,
            [
                "Work-item note missing: /notes/pwf/PWF-0001.md".to_string(),
                ISSUE_PLACEHOLDER_PROMPT.to_string(),
            ]
        );
        assert_eq!(enriched.prompt, "");
        assert_eq!(enriched.format, "file");
        assert_eq!(
            enriched.item_file.as_deref(),
            Some("/notes/pwf/PWF-0001.md")
        );
    }

    #[test]
    fn inline_legacy_record_projects_legacy_format_and_ordinal_id() {
        let inline = PendingWorkItem {
            id: RecordId::Inline(2),
            title: "legacy task".to_string(),
            body: "do the legacy thing".to_string(),
            source: "do the legacy thing".to_string(),
            locator: "/notes/pwf/pwf.md".to_string(),
            materialization: Materialization::InlineLegacy,
            ..record("")
        };

        let open = enrich(&inline, Some("/repo")).into_open_item("pwf".to_string());

        assert_eq!(open.id, "pwf:2");
        assert_eq!(open.format, "legacy");
        assert_eq!(open.item_file, None);
        assert_eq!(open.session, "legacy task");
        assert_eq!(open.prompt, "do the legacy thing");
        assert!(open.launchable);
    }

    #[test]
    fn empty_title_falls_back_to_id() {
        let mut rec = record("body");
        rec.title = "  ".to_string();
        assert_eq!(enrich(&rec, Some("/repo")).session, "PWF-0001");
    }

    #[test]
    fn section_label_is_normalized() {
        let mut rec = record("body");
        rec.section = Some("Futuro".to_string());
        assert_eq!(
            enrich(&rec, Some("/repo")).section.as_deref(),
            Some("Future")
        );
    }

    #[test]
    fn derive_flags_orders_issues_no_repo_missing_note_placeholder() {
        let flags = derive_flags(None, "", Some("/notes/pwf/PWF-0009.md"));
        assert_eq!(
            flags.issues,
            [
                ISSUE_NO_REPO.to_string(),
                "Work-item note missing: /notes/pwf/PWF-0009.md".to_string(),
                ISSUE_PLACEHOLDER_PROMPT.to_string(),
            ]
        );
    }
}
