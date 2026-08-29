//! Derives task launchability diagnostics for task views.

use std::num::NonZeroUsize;

use pwf_models::{
    AppDate,
    project::{ProjectName, ProjectSourceValue},
    task::{
        EffortTier, EffortTierError, PriorityTier, PriorityTierError, TaskId, TaskPrompt,
        TaskSection, TaskStatus, TaskTimestamp, TaskTitle, TaskTitleError,
    },
};
use pwf_wire::task::{
    BlockedByIssue, RawTaskTags, TaskHeading, TaskIndexPath, TaskIssue, TaskLaunch, TaskLocation,
    TaskNotePath, TaskView,
};

use super::{normalize_section_label, note_body::is_placeholder_prompt};
use crate::ports::task_record::{Materialization, StoredBlockedBy, TaskRecord};

/// Contains launchability flags and diagnostics derived from a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::task) struct DerivedFlags {
    pub(in crate::task) launch: TaskLaunch,
}

/// Derives missing-note and placeholder diagnostics.
#[must_use]
pub(in crate::task) fn derive_flags(
    prompt: &TaskPrompt,
    missing_note: Option<&TaskNotePath>,
) -> DerivedFlags {
    let mut issues = Vec::new();
    if let Some(path) = missing_note {
        issues.push(TaskIssue::MissingNote { path: path.clone() });
    }
    if is_placeholder_prompt(prompt) {
        issues.push(TaskIssue::PlaceholderPrompt);
    }
    DerivedFlags {
        launch: TaskLaunch::from_issues(issues),
    }
}

#[derive(Debug, thiserror::Error)]
pub(in crate::task) enum TaskViewError {
    #[error("task {id} has an invalid title: {source}")]
    Title {
        id: TaskId,
        #[source]
        source: TaskTitleError,
    },
    #[error("task {id} has an invalid effort value {value:?}: {source}")]
    Effort {
        id: TaskId,
        value: String,
        #[source]
        source: EffortTierError,
    },
    #[error("task {id} has an invalid priority value {value:?}: {source}")]
    Priority {
        id: TaskId,
        value: String,
        #[source]
        source: PriorityTierError,
    },
}

/// Contains a task's persisted data and derived diagnostics before project
/// attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::task) struct EnrichedTask {
    pub(in crate::task) id: TaskId,
    pub(in crate::task) status: TaskStatus,
    pub(in crate::task) heading: TaskHeading,
    pub(in crate::task) prompt: TaskPrompt,
    pub(in crate::task) project_path: ProjectSourceValue,
    pub(in crate::task) location: TaskLocation,
    pub(in crate::task) launch: TaskLaunch,
    pub(in crate::task) section: Option<TaskSection>,
    pub(in crate::task) blocked_by: Option<pwf_models::task::BlockedBy>,
    pub(in crate::task) blocked_by_issues: Vec<BlockedByIssue>,
    pub(in crate::task) effort: Option<EffortTier>,
    pub(in crate::task) priority: Option<PriorityTier>,
    pub(in crate::task) tags: Option<RawTaskTags>,
    pub(in crate::task) created: Option<AppDate>,
}

impl EnrichedTask {
    /// Attaches the managed project name.
    #[must_use]
    pub(in crate::task) fn into_task_view(self, project: ProjectName) -> TaskView {
        TaskView {
            id: self.id,
            project,
            status: self.status,
            heading: self.heading,
            prompt: self.prompt,
            project_path: self.project_path,
            location: self.location,
            launch: self.launch,
            section: self.section,
            blocked_by: self.blocked_by,
            blocked_by_statuses: Vec::new(),
            blocked_by_issues: self.blocked_by_issues,
            effort: self.effort,
            priority: self.priority,
            tags: self.tags,
            created: self.created,
        }
    }
}

/// Projects a persisted task into the fields consumed by list and session.
///
/// Missing notes add an issue, empty titles fall back to the task ID, and section labels are
/// normalized for display.
pub(in crate::task) fn enrich(
    task: &TaskRecord,
    project_path: &ProjectSourceValue,
) -> Result<EnrichedTask, TaskViewError> {
    let prompt = TaskPrompt::new(task.body.trim());
    let missing_note = match &task.materialization {
        Materialization::NoteFile => None,
        Materialization::MissingNote { expected } => Some(expected),
    };
    let flags = derive_flags(&prompt, missing_note);
    let heading = if task.title.trim().is_empty() {
        TaskHeading::Identifier(task.id.clone())
    } else {
        TaskHeading::Title(TaskTitle::try_new(&task.title).map_err(|source| {
            TaskViewError::Title {
                id: task.id.clone(),
                source,
            }
        })?)
    };
    let (index_path, line) = task.placement.as_ref().map_or_else(
        || {
            (
                TaskIndexPath::new(task.locator.as_path().to_path_buf()),
                NonZeroUsize::MIN,
            )
        },
        |placement| (placement.index_path.clone(), placement.line),
    );
    let location = TaskLocation::new(index_path, line);
    let effort = task
        .effort
        .as_deref()
        .map(str::trim)
        .map(str::parse)
        .transpose()
        .map_err(|source| TaskViewError::Effort {
            id: task.id.clone(),
            value: task.effort.clone().unwrap_or_default(),
            source,
        })?;
    let priority = task
        .priority
        .as_deref()
        .map(str::trim)
        .map(str::parse)
        .transpose()
        .map_err(|source| TaskViewError::Priority {
            id: task.id.clone(),
            value: task.priority.clone().unwrap_or_default(),
            source,
        })?;
    let (blocked_by, blocked_by_issues) = match &task.blocked_by {
        StoredBlockedBy::Absent => (None, Vec::new()),
        StoredBlockedBy::Valid(blocked_by) => (Some(blocked_by.clone()), Vec::new()),
        StoredBlockedBy::Malformed { raw, reason } => (
            None,
            vec![BlockedByIssue::Malformed {
                path: task.locator.clone(),
                raw: raw.clone(),
                reason: reason.clone(),
            }],
        ),
    };
    Ok(EnrichedTask {
        id: task.id.clone(),
        status: task.status,
        heading,
        prompt,
        project_path: project_path.clone(),
        location,
        launch: flags.launch,
        section: task.section.as_ref().map(normalize_section_label),
        blocked_by,
        blocked_by_issues,
        effort,
        priority,
        tags: task.tags.clone(),
        created: task.created_at.map(TaskTimestamp::date),
    })
}

#[cfg(test)]
mod tests {
    use pwf_models::project::ProjectSourceValue;

    use super::*;
    use crate::{
        ports::task_record::{IndexPlacement, StoredBlockedBy},
        testing::task_record,
    };

    fn record(body: &str) -> TaskRecord {
        TaskRecord {
            body: body.to_string(),
            source: String::new(),
            locator: TaskNotePath::new("/notes/foo/FOO-0001.md".into()),
            placement: Some(IndexPlacement {
                index_path: TaskIndexPath::new("/notes/foo/foo.md".into()),
                line: NonZeroUsize::new(7).unwrap(),
            }),
            ..task_record("FOO-0001")
        }
    }

    fn project_path() -> ProjectSourceValue {
        ProjectSourceValue::try_new("/project").unwrap()
    }

    #[test]
    fn launchable_when_project_path_is_present_and_prompt_is_real() {
        let enriched = enrich(&record("add startup toggle"), &project_path()).unwrap();
        assert!(enriched.launch.is_ready());
        assert!(!enriched.launch.needs_prompt());
        assert!(enriched.launch.issues().is_empty());
        assert_eq!(enriched.prompt.as_ref(), "add startup toggle");
        assert_eq!(enriched.project_path.as_ref(), "/project");
    }

    #[test]
    fn note_and_line_render_the_index_placement_not_the_note_file() {
        let enriched = enrich(&record("body"), &project_path()).unwrap();
        assert_eq!(
            enriched.location.index_path().as_path(),
            std::path::Path::new("/notes/foo/foo.md")
        );
        assert_eq!(enriched.location.line().get(), 7);
    }

    #[test]
    fn placeholder_prompt_is_not_launchable() {
        let enriched = enrich(&record("TODO"), &project_path()).unwrap();
        assert!(!enriched.launch.is_ready());
        assert!(enriched.launch.needs_prompt());
        assert_eq!(enriched.launch.issues(), [TaskIssue::PlaceholderPrompt]);
    }

    #[test]
    fn missing_note_wikilink_needs_attention_with_missing_note_issue() {
        let mut rec = record("");
        rec.materialization = Materialization::MissingNote {
            expected: TaskNotePath::new("/notes/foo/FOO-0001.md".into()),
        };

        let enriched = enrich(&rec, &project_path()).unwrap();

        assert!(!enriched.launch.is_ready());
        assert!(enriched.launch.needs_prompt());
        assert_eq!(
            enriched.launch.issues(),
            [
                TaskIssue::MissingNote {
                    path: TaskNotePath::new("/notes/foo/FOO-0001.md".into()),
                },
                TaskIssue::PlaceholderPrompt,
            ]
        );
        assert_eq!(enriched.prompt.as_ref(), "");
    }

    #[test]
    fn malformed_blocked_by_is_an_observation_not_a_launch_blocker() {
        let mut rec = record("body");
        rec.blocked_by = StoredBlockedBy::Malformed {
            raw: "\"[[AUX-0001]]\"".to_string(),
            reason: "expected a sequence".to_string(),
        };

        let enriched = enrich(&rec, &project_path()).unwrap();

        assert!(enriched.launch.is_ready());
        assert!(matches!(
            enriched.blocked_by_issues.as_slice(),
            [BlockedByIssue::Malformed { raw, .. }] if raw == "\"[[AUX-0001]]\""
        ));
    }

    #[test]
    fn empty_title_falls_back_to_id() {
        let mut rec = record("body");
        rec.title = "  ".to_string();
        assert_eq!(
            enrich(&rec, &project_path()).unwrap().heading.as_ref(),
            "FOO-0001"
        );
    }

    #[test]
    fn section_label_is_normalized() {
        let mut rec = record("body");
        rec.section = Some("Futuro".parse().unwrap());
        assert_eq!(
            enrich(&rec, &project_path())
                .unwrap()
                .section
                .as_ref()
                .map(AsRef::as_ref),
            Some("Future")
        );
    }

    #[test]
    fn derive_flags_orders_missing_note_before_placeholder() {
        let missing = TaskNotePath::new("/notes/foo/FOO-0009.md".into());
        let flags = derive_flags(&TaskPrompt::default(), Some(&missing));
        assert_eq!(
            flags.launch.issues(),
            [
                TaskIssue::MissingNote {
                    path: TaskNotePath::new("/notes/foo/FOO-0009.md".into()),
                },
                TaskIssue::PlaceholderPrompt,
            ]
        );
    }

    #[test]
    fn invalid_persisted_effort_does_not_enter_a_task_view() {
        let mut rec = record("body");
        rec.effort = Some("extreme".to_string());

        assert!(matches!(
            enrich(&rec, &project_path()),
            Err(TaskViewError::Effort { ref value, .. }) if value == "extreme"
        ));
    }

    #[test]
    fn invalid_persisted_priority_does_not_enter_a_task_view() {
        let mut rec = record("body");
        rec.priority = Some("urgent".to_string());

        assert!(matches!(
            enrich(&rec, &project_path()),
            Err(TaskViewError::Priority { ref value, .. }) if value == "urgent"
        ));
    }

    #[test]
    fn oversized_persisted_title_does_not_enter_a_task_view() {
        let mut rec = record("body");
        rec.title = "x".repeat(201);

        let error = enrich(&rec, &project_path()).unwrap_err();

        assert!(matches!(error, TaskViewError::Title { .. }));
    }
}
