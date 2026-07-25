//! Derives pending-work launchability diagnostics for list, verify, and session operations.

use pwf_domain::pending_work::WorkItemStatus;

use super::{list::PendingWorkItemView, note_body::is_placeholder_prompt, section};
use crate::{Materialization, PendingWorkItem, RecordId};

pub const ISSUE_NO_REPO: &str =
    "Project has no directory source; update the managed project record.";
pub const ISSUE_PLACEHOLDER_PROMPT: &str =
    "Prompt is a placeholder; define a real prompt before launching.";

/// Canonicalizes known section aliases and trims unknown labels for display.
#[must_use]
pub(super) fn normalize_section_label(label: &str) -> String {
    section::alias(label).map_or_else(|| label.trim().to_string(), str::to_string)
}

#[must_use]
pub(super) fn normalize_section(section: Option<&str>) -> Option<String> {
    section.map(normalize_section_label)
}

/// Returns whether an item is active and represented by an open index placement.
#[must_use]
pub(crate) fn is_open_item(item: &PendingWorkItem) -> bool {
    item.status == WorkItemStatus::Active && item.placement.is_some()
}

/// Contains launchability flags and diagnostics derived from a pending-work item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedFlags {
    pub issues: Vec<String>,
    pub launchable: bool,
    pub needs_prompt: bool,
}

/// Derives diagnostics in repository, missing-note, placeholder order.
#[must_use]
pub(super) fn derive_flags(
    repo: Option<&str>,
    prompt: &str,
    missing_note: Option<&str>,
) -> DerivedFlags {
    let mut issues = Vec::new();
    if repo.is_none_or(|value| value.trim().is_empty()) {
        issues.push(ISSUE_NO_REPO.to_string());
    }
    if let Some(path) = missing_note {
        issues.push(missing_note_issue(path));
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

/// Contains a pending-work item's persisted data and derived diagnostics before project attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrichedPendingWorkItem {
    pub id: RecordId,
    pub status: WorkItemStatus,
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

impl EnrichedPendingWorkItem {
    /// Attaches the project and composes inline ids as `<project>:<ordinal>`.
    #[must_use]
    pub fn into_pending_work_item_view(self, project: String) -> PendingWorkItemView {
        let id = match &self.id {
            RecordId::Item(id) => id.as_ref().to_string(),
            RecordId::Inline(ordinal) => inline_record_id(&project, *ordinal),
        };
        PendingWorkItemView {
            id,
            project,
            status: self.status,
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
            prerequisite_statuses: Vec::new(),
            effort: self.effort,
            tags: self.tags,
            created: self.created,
        }
    }
}

fn missing_note_issue(path: &str) -> String {
    format!("Work-item note missing: {path}")
}

pub(crate) fn inline_record_id(project: &str, ordinal: usize) -> String {
    format!("{project}:{ordinal}")
}

/// Projects a persisted item into the fields consumed by list, verify, and session.
///
/// Materialization controls `format` and `item_file`; missing notes add an issue; empty titles fall
/// back to the canonical id; section labels are canonicalized for display.
#[must_use]
pub(super) fn enrich(item: &PendingWorkItem, repo: Option<&str>) -> EnrichedPendingWorkItem {
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
    EnrichedPendingWorkItem {
        id: item.id.clone(),
        status: item.status,
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
    fn open_item_requires_active_status_and_index_placement() {
        let active = record("body");
        assert!(is_open_item(&active));

        let unlinked = PendingWorkItem {
            placement: None,
            ..active.clone()
        };
        assert!(!is_open_item(&unlinked));

        let done = PendingWorkItem {
            status: WorkItemStatus::Done,
            ..active
        };
        assert!(!is_open_item(&done));
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

        let item = enrich(&inline, Some("/repo")).into_pending_work_item_view("pwf".to_string());

        assert_eq!(item.id, "pwf:2");
        assert_eq!(item.format, "legacy");
        assert_eq!(item.item_file, None);
        assert_eq!(item.session, "legacy task");
        assert_eq!(item.prompt, "do the legacy thing");
        assert!(item.launchable);
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
    fn unknown_section_label_preserves_trimmed_case() {
        assert_eq!(normalize_section_label(" SomeDay "), "SomeDay");
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
