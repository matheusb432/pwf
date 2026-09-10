//! Projects stored task metadata and derives launch diagnostics.

use pwf_models::{
    project::{ProjectName, ProjectSourceValue},
    task::{
        EffortTier, EffortTierError, PriorityTier, PriorityTierError, TaskId, TaskPrompt,
        TaskTimestamp, TaskTitle, TaskTitleError,
    },
};
use pwf_wire::task::{
    BlockedByIssue, ListedTask, ListedTaskDetails, StoredBlockedBy, TaskHeading, TaskIssue,
    TaskLaunch, TaskRecord,
};

use super::note_body::is_placeholder_prompt;
use crate::ports::task_vault::TaskSummaryRecord;

/// Derives placeholder diagnostics.
#[must_use]
pub(in crate::task) fn derive_flags(prompt: &TaskPrompt) -> TaskLaunch {
    let mut issues = Vec::new();
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
/// Empty titles fall back to the task ID.
pub(in crate::task) fn detailed(
    task: &TaskRecord,
    project: ProjectName,
    project_path: Option<&ProjectSourceValue>,
) -> Result<ListedTask, TaskProjectionError> {
    let prompt = TaskPrompt::new(task.body.trim());
    let flags = derive_flags(&prompt);
    let heading = task_heading(&task.id, &task.title)?;
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
        effort,
        priority,
        tags: task.tags.clone(),
        created: task.created_at.map(TaskTimestamp::date),
        details: Some(ListedTaskDetails {
            prompt,
            project_path: project_path.cloned(),
            note_path: task.locator.clone(),
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
    use pwf_wire::task::{StoredBlockedBy, TaskNotePath};

    use super::*;
    use crate::testing::task_record;

    fn record(body: &str) -> TaskRecord {
        TaskRecord {
            body: body.to_string(),
            source: String::new(),
            locator: TaskNotePath::new("/notes/foo/FOO-0001.md".into()),
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
    fn detailed_task_points_to_its_note_file() {
        let enriched = detailed(
            &record("body"),
            ProjectName::try_new("foo").unwrap(),
            Some(&project_path()),
        )
        .unwrap();
        assert_eq!(
            enriched.details.as_ref().unwrap().note_path.as_path(),
            std::path::Path::new("/notes/foo/FOO-0001.md")
        );
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
