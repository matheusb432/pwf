//! Protobuf response mappings for task operations.

use pwf_models::task::{EffortTier, TaskStatus};

use crate::{confirmation, task, v1};

#[must_use]
pub fn add_task_response(task: task::AddedTask) -> v1::AddTaskResponse {
    v1::AddTaskResponse {
        id: task.id.to_string(),
        project: task.project.to_string(),
        title: task.title.to_string(),
        note_path: task.note_path.to_string(),
        created_section: task.created_section.map(|section| section.to_string()),
    }
}

pub fn add_task_failure_details(
    diagnostics: &task::AddTaskDiagnostics,
) -> v1::AddTaskFailureDetails {
    v1::AddTaskFailureDetails {
        project: diagnostics.project.to_string(),
        created_section: diagnostics
            .created_section
            .as_ref()
            .map(ToString::to_string),
    }
}

#[must_use]
pub fn edit_task_response(task: task::EditedTask) -> v1::EditTaskResponse {
    let task::EditedTask { id, project, title } = task;
    v1::EditTaskResponse {
        id: id.to_string(),
        project: project.to_string(),
        title: title.to_string(),
    }
}

pub fn cancel_task_response(task: task::ClosedTask) -> v1::CancelTaskResponse {
    v1::CancelTaskResponse {
        id: task.id.to_string(),
        project: task.project.to_string(),
        title: task.title.to_string(),
        action: closed_task_action(task.action),
        evicted_ids: task
            .evicted_ids
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
        futuro_renamed_project: task
            .futuro_renamed_project
            .map(|project| project.to_string()),
        review_task: task.review_task.map(add_task_response),
    }
}

pub fn complete_task_response(task: task::ClosedTask) -> v1::CompleteTaskResponse {
    v1::CompleteTaskResponse {
        id: task.id.to_string(),
        project: task.project.to_string(),
        title: task.title.to_string(),
        action: closed_task_action(task.action),
        evicted_ids: task
            .evicted_ids
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
        futuro_renamed_project: task
            .futuro_renamed_project
            .map(|project| project.to_string()),
        review_task: task.review_task.map(add_task_response),
    }
}

#[must_use]
pub fn reopen_task_result(task: task::ReopenedTask) -> v1::ReopenedTask {
    match task {
        task::ReopenedTask::Reopened { id, project } => v1::ReopenedTask {
            outcome: v1::ReopenedTaskOutcome::Reopened as i32,
            id: id.to_string(),
            project: Some(project.to_string()),
        },
        task::ReopenedTask::AlreadyActive { id, project } => v1::ReopenedTask {
            outcome: v1::ReopenedTaskOutcome::AlreadyActive as i32,
            id: id.to_string(),
            project: Some(project.to_string()),
        },
        task::ReopenedTask::Aborted { id } => v1::ReopenedTask {
            outcome: v1::ReopenedTaskOutcome::Aborted as i32,
            id: id.to_string(),
            project: None,
        },
    }
}

#[must_use]
pub fn remove_task_result(task: task::RemovedTaskOutcome) -> v1::RemovedTaskOutcome {
    match task {
        task::RemovedTaskOutcome::Removed(value) => v1::RemovedTaskOutcome {
            outcome: v1::RemovedTaskOutcomeKind::Removed as i32,
            task_id: value.id.to_string(),
            removed: Some(v1::RemovedTask {
                id: value.id.to_string(),
                project: value.project.to_string(),
                title: value.title.to_string(),
                deleted_path: value.deleted_path.to_string(),
                unlinked: value.unlinked.map(|path| path.to_string()),
            }),
        },
        task::RemovedTaskOutcome::Aborted { task_id } => v1::RemovedTaskOutcome {
            outcome: v1::RemovedTaskOutcomeKind::Aborted as i32,
            task_id: task_id.to_string(),
            removed: None,
        },
    }
}

#[must_use]
pub fn get_task_response(task: task::TaskRead) -> v1::GetTaskResponse {
    let value = match task {
        task::TaskRead::Markdown(value) => v1::get_task_response::Value::Markdown(value),
        task::TaskRead::Path(value) => v1::get_task_response::Value::Path(value.to_string()),
        task::TaskRead::Data(value) => {
            v1::get_task_response::Value::Data(Box::new(task_data(*value)))
        }
    };
    v1::GetTaskResponse { value: Some(value) }
}

pub fn list_tasks_response(tasks: task::ListedTasks) -> v1::ListTasksResponse {
    let status_filter = match tasks.status_filter {
        task::StatusFilter::Exact(TaskStatus::Active) => v1::TaskStatusFilter::Active,
        task::StatusFilter::Exact(TaskStatus::Done) => v1::TaskStatusFilter::Done,
        task::StatusFilter::Exact(TaskStatus::Cancelled) => v1::TaskStatusFilter::Cancelled,
        task::StatusFilter::All => v1::TaskStatusFilter::All,
    };
    let layout = match tasks.layout {
        task::ListLayout::Flat => v1::ListLayout::Flat,
        task::ListLayout::BySection => v1::ListLayout::BySection,
    };
    let detail = match tasks.detail {
        task::ListDetail::Summary => v1::ListDetail::Summary,
        task::ListDetail::Detailed => v1::ListDetail::Detailed,
    };
    v1::ListTasksResponse {
        tasks: tasks.tasks.into_iter().map(task_view).collect(),
        hidden: tasks.hidden as u64,
        project: tasks.project.map(|project| project.to_string()),
        project_task_path: tasks.project_task_path.map(|path| path.to_string()),
        status_filter: status_filter as i32,
        layout: layout as i32,
        detail: detail as i32,
    }
}

#[must_use]
pub fn remove_task_confirmation(
    confirmation: &confirmation::RemoveTaskConfirmation,
) -> v1::RemoveTaskConfirmation {
    v1::RemoveTaskConfirmation {
        task_id: confirmation.task_identifier.to_string(),
        project: confirmation.project.to_string(),
        title: confirmation.title.to_string(),
        status: task_status_value(confirmation.status),
        note_path: confirmation.note_path.to_string(),
    }
}

#[must_use]
pub fn reopen_task_confirmation(
    confirmation: &confirmation::ReopenTaskConfirmation,
) -> v1::ReopenTaskConfirmation {
    v1::ReopenTaskConfirmation {
        task_id: confirmation.task_identifier.to_string(),
        project: confirmation.project.to_string(),
        completion_date: confirmation.completion_date.map(|date| date.to_string()),
        commits: confirmation.commit_provenance.clone(),
        report: confirmation.report.clone(),
    }
}

pub(crate) fn blocked_by_status(status: task::BlockedByStatus) -> v1::BlockedByStatus {
    let (resolution, task_status, reason) = match status.resolution {
        task::BlockedByResolution::Found(status) => (
            v1::BlockedByResolutionKind::Found,
            Some(task_status_value(status)),
            None,
        ),
        task::BlockedByResolution::Missing => (v1::BlockedByResolutionKind::Missing, None, None),
        task::BlockedByResolution::Unavailable { reason } => {
            (v1::BlockedByResolutionKind::Unavailable, None, Some(reason))
        }
    };
    v1::BlockedByStatus {
        id: status.id.to_string(),
        title: status.title,
        resolution: resolution as i32,
        status: task_status,
        reason,
    }
}

pub(crate) fn blocked_by_issue(issue: task::BlockedByIssue) -> v1::BlockedByIssue {
    match issue {
        task::BlockedByIssue::Malformed { path, raw, reason } => v1::BlockedByIssue {
            path: path.to_string(),
            raw,
            reason,
        },
    }
}

fn task_data(task: task::TaskData) -> v1::TaskData {
    v1::TaskData {
        id: task.id.to_string(),
        project: task.project.to_string(),
        title: task.title.to_string(),
        status: task_status_value(task.status),
        created: task.created.map(|date| date.to_string()),
        completed: task.completed.map(|date| date.to_string()),
        commits: task.commits.map(|commits| commits.to_string()),
        tags: task
            .tags
            .map(|tags| tags.iter().map(ToString::to_string).collect())
            .unwrap_or_default(),
        effort: task.effort.map(effort_tier_value),
        blocked_by: task
            .blocked_by
            .map(|blocked_by| blocked_by.iter().map(ToString::to_string).collect())
            .unwrap_or_default(),
        section: task.section.map(|section| section.to_string()),
        prompt: task.prompt.to_string(),
    }
}

fn task_view(task: task::TaskView) -> v1::TaskView {
    v1::TaskView {
        id: task.id.to_string(),
        project: task.project.to_string(),
        status: task_status_value(task.status),
        heading: task.heading.to_string(),
        prompt: task.prompt.to_string(),
        project_path: task.project_path.to_string(),
        location: Some(v1::TaskLocation {
            index_path: task.location.index_path().to_string(),
            line: task.location.line().get() as u64,
        }),
        launch_issues: task.launch.issues().iter().map(task_issue).collect(),
        section: task.section.map(|section| section.to_string()),
        blocked_by: task
            .blocked_by
            .map(|blocked_by| blocked_by.iter().map(ToString::to_string).collect())
            .unwrap_or_default(),
        blocked_by_statuses: task
            .blocked_by_statuses
            .into_iter()
            .map(blocked_by_status)
            .collect(),
        blocked_by_issues: task
            .blocked_by_issues
            .into_iter()
            .map(blocked_by_issue)
            .collect(),
        effort: task.effort.map(effort_tier_value),
        raw_tags: task.tags.map(|tags| tags.to_string()),
        created: task.created.map(|date| date.to_string()),
    }
}

fn task_issue(issue: &task::TaskIssue) -> v1::TaskIssue {
    match issue {
        task::TaskIssue::MissingNote { path } => v1::TaskIssue {
            kind: v1::TaskIssueKind::MissingNote as i32,
            path: Some(path.to_string()),
        },
        task::TaskIssue::PlaceholderPrompt => v1::TaskIssue {
            kind: v1::TaskIssueKind::PlaceholderPrompt as i32,
            path: None,
        },
    }
}

fn closed_task_action(action: task::ClosedTaskAction) -> i32 {
    match action {
        task::ClosedTaskAction::Done => v1::ClosedTaskAction::Done as i32,
        task::ClosedTaskAction::Cancelled => v1::ClosedTaskAction::Cancelled as i32,
    }
}

fn task_status_value(status: TaskStatus) -> i32 {
    match status {
        TaskStatus::Active => v1::TaskStatus::Active as i32,
        TaskStatus::Done => v1::TaskStatus::Done as i32,
        TaskStatus::Cancelled => v1::TaskStatus::Cancelled as i32,
    }
}

fn effort_tier_value(effort: EffortTier) -> i32 {
    match effort {
        EffortTier::Low => v1::EffortTier::Low as i32,
        EffortTier::Medium => v1::EffortTier::Medium as i32,
        EffortTier::High => v1::EffortTier::High as i32,
        EffortTier::Highest => v1::EffortTier::Highest as i32,
    }
}
