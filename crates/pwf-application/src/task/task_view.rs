//! Derives task launchability diagnostics for task views.

use pwf_models::{
    project::ProjectSourceValue,
    task::{TaskId, TaskStatus},
};
use pwf_wire::task::TaskView;

use super::{normalize_section_label, note_body::is_placeholder_prompt, prerequisites};
use crate::ports::task_record::{Materialization, TaskRecord};

pub(in crate::task) const ISSUE_PLACEHOLDER_PROMPT: &str =
    "Prompt is a placeholder; define a real prompt before launching.";

#[must_use]
pub(in crate::task) fn normalize_section(section: Option<&str>) -> Option<String> {
    section.map(normalize_section_label)
}

/// Returns whether an task is active.
#[must_use]
pub(in crate::task) fn is_active_task(task: &TaskRecord) -> bool {
    task.status == TaskStatus::Active
}

/// Contains launchability flags and diagnostics derived from a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::task) struct DerivedFlags {
    pub(in crate::task) issues: Vec<String>,
    pub(in crate::task) launchable: bool,
    pub(in crate::task) needs_prompt: bool,
}

/// Derives missing-note and placeholder diagnostics.
#[must_use]
pub(in crate::task) fn derive_flags(prompt: &str, missing_note: Option<&str>) -> DerivedFlags {
    let mut issues = Vec::new();
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

/// Contains a task's persisted data and derived diagnostics before project
/// attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::task) struct EnrichedTask {
    pub(in crate::task) id: TaskId,
    pub(in crate::task) status: TaskStatus,
    pub(in crate::task) session: String,
    pub(in crate::task) prompt: String,
    pub(in crate::task) project_path: ProjectSourceValue,
    pub(in crate::task) note: String,
    pub(in crate::task) task_file: Option<String>,
    pub(in crate::task) line: usize,
    pub(in crate::task) format: String,
    pub(in crate::task) launchable: bool,
    pub(in crate::task) needs_prompt: bool,
    pub(in crate::task) issues: Vec<String>,
    pub(in crate::task) section: Option<String>,
    pub(in crate::task) prerequisites: Option<pwf_models::task::Prerequisites>,
    pub(in crate::task) effort: Option<String>,
    pub(in crate::task) tags: Option<String>,
    pub(in crate::task) created: Option<String>,
}

impl EnrichedTask {
    /// Attaches the managed project name.
    #[must_use]
    pub(in crate::task) fn into_task_view(self, project: String) -> TaskView {
        TaskView {
            id: self.id,
            project,
            status: self.status,
            session: self.session,
            prompt: self.prompt,
            project_path: self.project_path,
            note: self.note,
            task_file: self.task_file,
            line: self.line,
            format: self.format,
            launchable: self.launchable,
            needs_prompt: self.needs_prompt,
            issues: self.issues,
            section: self.section,
            prerequisites: self.prerequisites,
            prerequisite_statuses: Vec::new(),
            effort: self.effort,
            tags: self.tags,
            created: self.created,
        }
    }
}

fn missing_note_issue(path: &str) -> String {
    format!("Task note missing: {path}")
}

/// Projects a persisted task into the fields consumed by list and session.
///
/// Materialization controls `format` and `task_file`; missing notes add an issue; empty titles
/// fall back to the task ID; section labels are normalized for display.
#[must_use]
pub(in crate::task) fn enrich(
    task: &TaskRecord,
    project_path: &ProjectSourceValue,
) -> EnrichedTask {
    let prompt = task.body.trim().to_string();
    let (format, task_file, missing_note) = match &task.materialization {
        Materialization::NoteFile => ("file", Some(task.locator.clone()), None),
        Materialization::MissingNote { expected } => {
            ("file", Some(task.locator.clone()), Some(expected.as_str()))
        }
    };
    let flags = derive_flags(&prompt, missing_note);
    let session = if task.title.trim().is_empty() {
        task.id.to_string()
    } else {
        task.title.clone()
    };
    let (note, line) = task.placement.as_ref().map_or_else(
        || (task.locator.clone(), 1),
        |placement| (placement.index_path.clone(), placement.line),
    );
    EnrichedTask {
        id: task.id.clone(),
        status: task.status,
        session,
        prompt,
        project_path: project_path.clone(),
        note,
        line,
        task_file,
        format: format.to_string(),
        launchable: flags.launchable,
        needs_prompt: flags.needs_prompt,
        issues: flags.issues,
        section: normalize_section(task.section.as_deref()),
        prerequisites: task.prereq.as_deref().and_then(prerequisites::extract),
        effort: task.effort.clone(),
        tags: task.tags.clone(),
        created: task.created.as_ref().map(|ts| ts.as_str().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use pwf_models::{project::ProjectSourceValue, task::TaskStatus};

    use super::*;
    use crate::{ports::task_record::IndexPlacement, testing::task_record};

    fn record(body: &str) -> TaskRecord {
        TaskRecord {
            body: body.to_string(),
            source: String::new(),
            locator: "/notes/pwf/PWF-0001.md".to_string(),
            placement: Some(IndexPlacement {
                index_path: "/notes/pwf/pwf.md".to_string(),
                line: 7,
            }),
            ..task_record("PWF-0001")
        }
    }

    fn project_path() -> ProjectSourceValue {
        ProjectSourceValue::try_new("/project").unwrap()
    }

    #[test]
    fn launchable_when_project_path_is_present_and_prompt_is_real() {
        let enriched = enrich(&record("add startup toggle"), &project_path());
        assert!(enriched.launchable);
        assert!(!enriched.needs_prompt);
        assert!(enriched.issues.is_empty());
        assert_eq!(enriched.prompt, "add startup toggle");
        assert_eq!(enriched.project_path.as_ref(), "/project");
    }

    #[test]
    fn active_item_requires_only_active_status() {
        let active = record("body");
        assert!(is_active_task(&active));

        let unlinked = TaskRecord {
            placement: None,
            ..active.clone()
        };
        assert!(is_active_task(&unlinked));

        let done = TaskRecord {
            status: TaskStatus::Done,
            ..active
        };
        assert!(!is_active_task(&done));
    }

    #[test]
    fn note_and_line_render_the_index_placement_not_the_note_file() {
        let enriched = enrich(&record("body"), &project_path());
        assert_eq!(enriched.note, "/notes/pwf/pwf.md");
        assert_eq!(enriched.line, 7);
        assert_eq!(
            enriched.task_file.as_deref(),
            Some("/notes/pwf/PWF-0001.md")
        );
        assert_eq!(enriched.format, "file");
    }

    #[test]
    fn placeholder_prompt_is_not_launchable() {
        let enriched = enrich(&record("TODO"), &project_path());
        assert!(!enriched.launchable);
        assert!(enriched.needs_prompt);
        assert_eq!(enriched.issues, [ISSUE_PLACEHOLDER_PROMPT.to_string()]);
    }

    #[test]
    fn missing_note_wikilink_needs_attention_with_missing_note_issue() {
        let mut rec = record("");
        rec.materialization = Materialization::MissingNote {
            expected: "/notes/pwf/PWF-0001.md".to_string(),
        };

        let enriched = enrich(&rec, &project_path());

        assert!(!enriched.launchable, "missing note must not be launchable");
        assert!(enriched.needs_prompt, "empty prompt is a placeholder");
        assert_eq!(
            enriched.issues,
            [
                "Task note missing: /notes/pwf/PWF-0001.md".to_string(),
                ISSUE_PLACEHOLDER_PROMPT.to_string(),
            ]
        );
        assert_eq!(enriched.prompt, "");
        assert_eq!(enriched.format, "file");
        assert_eq!(
            enriched.task_file.as_deref(),
            Some("/notes/pwf/PWF-0001.md")
        );
    }

    #[test]
    fn empty_title_falls_back_to_id() {
        let mut rec = record("body");
        rec.title = "  ".to_string();
        assert_eq!(enrich(&rec, &project_path()).session, "PWF-0001");
    }

    #[test]
    fn section_label_is_normalized() {
        let mut rec = record("body");
        rec.section = Some("Futuro".to_string());
        assert_eq!(
            enrich(&rec, &project_path()).section.as_deref(),
            Some("Future")
        );
    }

    #[test]
    fn derive_flags_orders_missing_note_before_placeholder() {
        let flags = derive_flags("", Some("/notes/pwf/PWF-0009.md"));
        assert_eq!(
            flags.issues,
            [
                "Task note missing: /notes/pwf/PWF-0009.md".to_string(),
                ISSUE_PLACEHOLDER_PROMPT.to_string(),
            ]
        );
    }
}
