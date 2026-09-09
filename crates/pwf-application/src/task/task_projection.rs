//! Projects stored task metadata and derives launch diagnostics.

use std::num::NonZeroUsize;

use pwf_models::{
    project::{ProjectName, ProjectSourceValue},
    task::{
        EffortTier, EffortTierError, PriorityTier, PriorityTierError, TaskId, TaskPrompt,
        TaskTimestamp, TaskTitle, TaskTitleError,
    },
};
use pwf_wire::task::{
    BlockedByIssue, ListedTask, ListedTaskDetails, Materialization, StoredBlockedBy, TaskHeading,
    TaskIndexPath, TaskIssue, TaskLaunch, TaskLocation, TaskNotePath, TaskRecord,
};

use super::note_body::is_placeholder_prompt;
use crate::ports::task_vault::TaskSummaryRecord;

/// Derives missing-note and placeholder diagnostics.
#[must_use]
pub(in crate::task) fn derive_flags(
    prompt: &TaskPrompt,
    missing_note: Option<&TaskNotePath>,
) -> TaskLaunch {
    let mut issues = Vec::new();
    if let Some(path) = missing_note {
        issues.push(TaskIssue::MissingNote { path: path.clone() });
    }
    if is_placeholder_prompt(prompt) {
        issues.push(TaskIssue::PlaceholderPrompt);
    }
    TaskLaunch::from_issues(issues)
}

#[derive(Debug, thiserror::Error)]
pub(in crate::task) enum TaskProjectionError {
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

/// Projects a stored task into a detailed list entry.
///
/// Missing notes add an issue, empty titles fall back to the task ID, and section labels retain
/// their index spelling.
pub(in crate::task) fn detailed(
    task: &TaskRecord,
    project: ProjectName,
    project_path: Option<&ProjectSourceValue>,
) -> Result<ListedTask, TaskProjectionError> {
    let prompt = TaskPrompt::new(task.body.trim());
    let missing_note = match &task.materialization {
        Materialization::NoteFile => None,
        Materialization::MissingNote { expected } => Some(expected),
    };
    let flags = derive_flags(&prompt, missing_note);
    let heading = task_heading(&task.id, &task.title)?;
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
    let effort = task_effort(&task.id, task.effort.as_deref())?;
    let priority = task_priority(&task.id, task.priority.as_deref())?;
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
    Ok(ListedTask {
        id: task.id.clone(),
        project,
        status: task.status,
        heading,
        section: task.section.clone(),
        effort,
        priority,
        tags: task.tags.clone(),
        created: task.created_at.map(TaskTimestamp::date),
        details: Some(ListedTaskDetails {
            prompt,
            project_path: project_path.cloned(),
            location,
            launch: flags,
            blocked_by,
            blocked_by_statuses: Vec::new(),
            blocked_by_issues,
        }),
    })
}

pub(in crate::task) fn summarize(
    task: TaskSummaryRecord,
    project: ProjectName,
) -> Result<pwf_wire::task::ListedTask, TaskProjectionError> {
    let heading = task_heading(&task.id, &task.title)?;
    let effort = task_effort(&task.id, task.effort.as_deref())?;
    let priority = task_priority(&task.id, task.priority.as_deref())?;
    Ok(pwf_wire::task::ListedTask {
        id: task.id,
        project,
        status: task.status,
        heading,
        section: task.section,
        effort,
        priority,
        tags: task.tags,
        created: task.created_at.map(TaskTimestamp::date),
        details: None,
    })
}

pub(super) fn task_heading(id: &TaskId, title: &str) -> Result<TaskHeading, TaskProjectionError> {
    if title.trim().is_empty() {
        Ok(TaskHeading::Identifier(id.clone()))
    } else {
        TaskTitle::try_new(title)
            .map(TaskHeading::Title)
            .map_err(|source| TaskProjectionError::Title {
                id: id.clone(),
                source,
            })
    }
}

pub(super) fn task_effort(
    id: &TaskId,
    value: Option<&str>,
) -> Result<Option<EffortTier>, TaskProjectionError> {
    value
        .map(str::trim)
        .map(str::parse)
        .transpose()
        .map_err(|source| TaskProjectionError::Effort {
            id: id.clone(),
            value: value.unwrap_or_default().to_string(),
            source,
        })
}

pub(super) fn task_priority(
    id: &TaskId,
    value: Option<&str>,
) -> Result<Option<PriorityTier>, TaskProjectionError> {
    value
        .map(str::trim)
        .map(str::parse)
        .transpose()
        .map_err(|source| TaskProjectionError::Priority {
            id: id.clone(),
            value: value.unwrap_or_default().to_string(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use pwf_models::project::ProjectSourceValue;
    use pwf_wire::task::{IndexPlacement, StoredBlockedBy};

    use super::*;
    use crate::testing::task_record;

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
        let enriched = detailed(
            &record("add startup toggle"),
            ProjectName::try_new("foo").unwrap(),
            Some(&project_path()),
        )
        .unwrap();
        assert!(enriched.details.as_ref().unwrap().launch.is_ready());
        assert!(!enriched.details.as_ref().unwrap().launch.needs_prompt());
        assert!(
            enriched
                .details
                .as_ref()
                .unwrap()
                .launch
                .issues()
                .is_empty()
        );
        assert_eq!(
            enriched.details.as_ref().unwrap().prompt.as_ref(),
            "add startup toggle"
        );
        assert_eq!(
            enriched
                .details
                .as_ref()
                .unwrap()
                .project_path
                .as_ref()
                .unwrap()
                .as_ref(),
            "/project"
        );
    }

    #[test]
    fn note_and_line_render_the_index_placement_not_the_note_file() {
        let enriched = detailed(
            &record("body"),
            ProjectName::try_new("foo").unwrap(),
            Some(&project_path()),
        )
        .unwrap();
        assert_eq!(
            enriched
                .details
                .as_ref()
                .unwrap()
                .location
                .index_path()
                .as_path(),
            std::path::Path::new("/notes/foo/foo.md")
        );
        assert_eq!(enriched.details.as_ref().unwrap().location.line().get(), 7);
    }

    #[test]
    fn placeholder_prompt_is_not_launchable() {
        let enriched = detailed(
            &record("TODO"),
            ProjectName::try_new("foo").unwrap(),
            Some(&project_path()),
        )
        .unwrap();
        assert!(!enriched.details.as_ref().unwrap().launch.is_ready());
        assert!(enriched.details.as_ref().unwrap().launch.needs_prompt());
        assert_eq!(
            enriched.details.as_ref().unwrap().launch.issues(),
            [TaskIssue::PlaceholderPrompt]
        );
    }

    #[test]
    fn missing_note_wikilink_needs_attention_with_missing_note_issue() {
        let mut rec = record("");
        rec.materialization = Materialization::MissingNote {
            expected: TaskNotePath::new("/notes/foo/FOO-0001.md".into()),
        };

        let enriched = detailed(
            &rec,
            ProjectName::try_new("foo").unwrap(),
            Some(&project_path()),
        )
        .unwrap();

        assert!(!enriched.details.as_ref().unwrap().launch.is_ready());
        assert!(enriched.details.as_ref().unwrap().launch.needs_prompt());
        assert_eq!(
            enriched.details.as_ref().unwrap().launch.issues(),
            [
                TaskIssue::MissingNote {
                    path: TaskNotePath::new("/notes/foo/FOO-0001.md".into()),
                },
                TaskIssue::PlaceholderPrompt,
            ]
        );
        assert_eq!(enriched.details.as_ref().unwrap().prompt.as_ref(), "");
    }

    #[test]
    fn malformed_blocked_by_is_an_observation_not_a_launch_blocker() {
        let mut rec = record("body");
        rec.blocked_by = StoredBlockedBy::Malformed {
            raw: "\"[[AUX-0001]]\"".to_string(),
            reason: "expected a sequence".to_string(),
        };

        let enriched = detailed(
            &rec,
            ProjectName::try_new("foo").unwrap(),
            Some(&project_path()),
        )
        .unwrap();

        assert!(enriched.details.as_ref().unwrap().launch.is_ready());
        assert!(matches!(
            enriched.details.as_ref().unwrap().blocked_by_issues.as_slice(),
            [BlockedByIssue::Malformed { raw, .. }] if raw == "\"[[AUX-0001]]\""
        ));
    }

    #[test]
    fn empty_title_falls_back_to_id() {
        let mut rec = record("body");
        rec.title = "  ".to_string();
        assert_eq!(
            detailed(
                &rec,
                ProjectName::try_new("foo").unwrap(),
                Some(&project_path())
            )
            .unwrap()
            .heading
            .as_ref(),
            "FOO-0001"
        );
    }

    #[test]
    fn section_label_retains_its_index_spelling() {
        let mut rec = record("body");
        rec.section = Some("Futuro".parse().unwrap());
        assert_eq!(
            detailed(
                &rec,
                ProjectName::try_new("foo").unwrap(),
                Some(&project_path())
            )
            .unwrap()
            .section
            .as_ref()
            .map(AsRef::as_ref),
            Some("Futuro")
        );
    }

    #[test]
    fn derive_flags_orders_missing_note_before_placeholder() {
        let missing = TaskNotePath::new("/notes/foo/FOO-0009.md".into());
        let flags = derive_flags(&TaskPrompt::default(), Some(&missing));
        assert_eq!(
            flags.issues(),
            [
                TaskIssue::MissingNote {
                    path: TaskNotePath::new("/notes/foo/FOO-0009.md".into()),
                },
                TaskIssue::PlaceholderPrompt,
            ]
        );
    }

    #[test]
    fn invalid_persisted_effort_does_not_enter_a_task_projection() {
        let mut rec = record("body");
        rec.effort = Some("extreme".to_string());

        assert!(matches!(
            detailed(&rec, ProjectName::try_new("foo").unwrap(), Some(&project_path())),
            Err(TaskProjectionError::Effort { ref value, .. }) if value == "extreme"
        ));
    }

    #[test]
    fn invalid_persisted_priority_does_not_enter_a_task_projection() {
        let mut rec = record("body");
        rec.priority = Some("urgent".to_string());

        assert!(matches!(
            detailed(&rec, ProjectName::try_new("foo").unwrap(), Some(&project_path())),
            Err(TaskProjectionError::Priority { ref value, .. }) if value == "urgent"
        ));
    }

    #[test]
    fn oversized_persisted_title_does_not_enter_a_task_projection() {
        let mut rec = record("body");
        rec.title = "x".repeat(201);

        let error = detailed(
            &rec,
            ProjectName::try_new("foo").unwrap(),
            Some(&project_path()),
        )
        .unwrap_err();

        assert!(matches!(error, TaskProjectionError::Title { .. }));
    }
}
