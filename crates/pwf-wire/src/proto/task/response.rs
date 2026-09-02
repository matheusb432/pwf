//! Protobuf response mappings for task operations.

use pwf_models::task::{EffortTier, PriorityTier, TaskId, TaskStatus};

use crate::{confirmation, pb, task};

#[must_use]
pub fn create_task_response(id: &TaskId) -> pb::CreateTaskResponse {
    pb::CreateTaskResponse { id: id.to_string() }
}

#[must_use]
pub fn update_task_response() -> pb::UpdateTaskResponse {
    pb::UpdateTaskResponse {}
}

#[must_use]
pub fn cancel_task_response(review_task_id: Option<TaskId>) -> pb::CancelTaskResponse {
    pb::CancelTaskResponse {
        review_task_id: review_task_id.map(|id| id.to_string()),
    }
}

#[must_use]
pub fn complete_task_response(review_task_id: Option<TaskId>) -> pb::CompleteTaskResponse {
    pb::CompleteTaskResponse {
        review_task_id: review_task_id.map(|id| id.to_string()),
    }
}

#[must_use]
pub fn reopen_task_result(outcome: task::ReopenTaskOutcome) -> pb::ReopenTaskResult {
    let outcome = match outcome {
        task::ReopenTaskOutcome::Reopened => {
            pb::reopen_task_result::Outcome::Reopened(pb::ReopenedTask {})
        }
        task::ReopenTaskOutcome::AlreadyActive => {
            pb::reopen_task_result::Outcome::AlreadyActive(pb::AlreadyActiveTask {})
        }
        task::ReopenTaskOutcome::Aborted => {
            pb::reopen_task_result::Outcome::Aborted(pb::AbortedTaskOperation {})
        }
    };
    pb::ReopenTaskResult {
        outcome: Some(outcome),
    }
}

#[must_use]
pub fn delete_task_result(outcome: task::DeleteTaskOutcome) -> pb::DeleteTaskResult {
    let outcome = match outcome {
        task::DeleteTaskOutcome::Deleted => {
            pb::delete_task_result::Outcome::Deleted(pb::DeletedTask {})
        }
        task::DeleteTaskOutcome::Aborted => {
            pb::delete_task_result::Outcome::Aborted(pb::AbortedTaskOperation {})
        }
    };
    pb::DeleteTaskResult {
        outcome: Some(outcome),
    }
}

#[must_use]
pub fn get_task_response(task: task::TaskSnapshot) -> pb::GetTaskResponse {
    let value = match task.value {
        task::TaskRead::Markdown(value) => pb::get_task_response::Value::Markdown(value),
        task::TaskRead::Path(value) => pb::get_task_response::Value::Path(value.to_string()),
        task::TaskRead::Data(value) => {
            pb::get_task_response::Value::Data(Box::new(task_data(*value)))
        }
    };
    pb::GetTaskResponse {
        revision: task.revision.to_string(),
        value: Some(value),
    }
}

/// Encodes a bounded native task DAG into its Protobuf response.
///
/// # Errors
///
/// Returns [`TaskDagResponseError`] when a native node index cannot fit the wire field.
pub fn get_task_dag_response(
    graph: task::TaskDag,
) -> Result<pb::GetTaskDagResponse, TaskDagResponseError> {
    Ok(pb::GetTaskDagResponse {
        root_id: graph.root_id.to_string(),
        nodes: graph.nodes.into_iter().map(task_dag_node).collect(),
        edges: graph
            .edges
            .into_iter()
            .map(|edge| {
                Ok(pb::TaskDagEdge {
                    blocker_node_index: u32::try_from(edge.blocker_node_index)
                        .map_err(|_| TaskDagResponseError)?,
                    dependent_node_index: u32::try_from(edge.dependent_node_index)
                        .map_err(|_| TaskDagResponseError)?,
                })
            })
            .collect::<Result<Vec<_>, TaskDagResponseError>>()?,
    })
}

/// Reports a native task-DAG index outside the Protobuf representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("task dependency graph contains an out-of-range node index")]
pub struct TaskDagResponseError;

fn task_dag_node(node: task::TaskDagNode) -> pb::TaskDagNode {
    let value = match node {
        task::TaskDagNode::Task { id, title, status } => {
            pb::task_dag_node::Value::Task(pb::TaskDagTaskNode {
                id: id.to_string(),
                title,
                status: task_status_value(status),
            })
        }
        task::TaskDagNode::Missing { id } => {
            pb::task_dag_node::Value::Missing(pb::TaskDagMissingNode { id: id.to_string() })
        }
        task::TaskDagNode::Unavailable { id } => {
            pb::task_dag_node::Value::Unavailable(pb::TaskDagUnavailableNode { id: id.to_string() })
        }
        task::TaskDagNode::DepthLimit => {
            pb::task_dag_node::Value::DepthLimit(pb::TaskDagDepthLimitNode {})
        }
    };
    pb::TaskDagNode { value: Some(value) }
}

pub fn list_tasks_response(tasks: task::ListedTasks) -> pb::ListTasksResponse {
    let next_page_token = tasks.next_page_token.as_ref().map(ToString::to_string);
    let status_filter = match tasks.status_filter {
        task::StatusFilter::Exact(TaskStatus::Active) => pb::TaskStatusFilter::Active,
        task::StatusFilter::Exact(TaskStatus::Done) => pb::TaskStatusFilter::Done,
        task::StatusFilter::Exact(TaskStatus::Cancelled) => pb::TaskStatusFilter::Cancelled,
        task::StatusFilter::All => pb::TaskStatusFilter::All,
    };
    let layout = match tasks.layout {
        task::ListLayout::Flat => pb::ListLayout::Flat,
        task::ListLayout::BySection => pb::ListLayout::BySection,
    };
    let detail = match tasks.detail {
        task::ListDetail::Summary => pb::ListDetail::Summary,
        task::ListDetail::Detailed => pb::ListDetail::Detailed,
    };
    pb::ListTasksResponse {
        tasks: tasks.tasks.into_iter().map(task_view).collect(),
        hidden: tasks.hidden as u64,
        project: tasks.project.map(|project| project.to_string()),
        project_task_path: tasks.project_task_path.map(|path| path.to_string()),
        status_filter: status_filter as i32,
        layout: layout as i32,
        detail: detail as i32,
        next_page_token,
    }
}

#[must_use]
pub fn delete_task_preflight(
    confirmation: &confirmation::RemoveTaskConfirmation,
) -> pb::DeleteTaskPreflight {
    pb::DeleteTaskPreflight {
        confirmation: Some(delete_task_confirmation(confirmation)),
    }
}

#[must_use]
pub fn reopen_task_preflight(
    confirmation: &confirmation::ReopenTaskConfirmation,
) -> pb::ReopenTaskPreflight {
    pb::ReopenTaskPreflight {
        confirmation: Some(reopen_task_confirmation(confirmation)),
    }
}

#[must_use]
pub fn delete_task_confirmation(
    confirmation: &confirmation::RemoveTaskConfirmation,
) -> pb::DeleteTaskConfirmation {
    pb::DeleteTaskConfirmation {
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
) -> pb::ReopenTaskConfirmation {
    pb::ReopenTaskConfirmation {
        task_id: confirmation.task_identifier.to_string(),
        project: confirmation.project.to_string(),
        completion_date: confirmation.completion_date.map(|date| date.to_string()),
        commits: confirmation.commit_provenance.clone(),
        report: confirmation.report.clone(),
    }
}

pub(crate) fn blocked_by_status(status: task::BlockedByStatus) -> pb::BlockedByStatus {
    let (resolution, task_status, reason) = match status.resolution {
        task::BlockedByResolution::Found(status) => (
            pb::BlockedByResolutionKind::Found,
            Some(task_status_value(status)),
            None,
        ),
        task::BlockedByResolution::Missing => (pb::BlockedByResolutionKind::Missing, None, None),
        task::BlockedByResolution::Unavailable { reason } => {
            (pb::BlockedByResolutionKind::Unavailable, None, Some(reason))
        }
    };
    pb::BlockedByStatus {
        id: status.id.to_string(),
        title: status.title,
        resolution: resolution as i32,
        status: task_status,
        reason,
    }
}

pub(crate) fn blocked_by_issue(issue: task::BlockedByIssue) -> pb::BlockedByIssue {
    match issue {
        task::BlockedByIssue::Malformed { path, raw, reason } => pb::BlockedByIssue {
            path: path.to_string(),
            raw,
            reason,
        },
    }
}

pub(super) fn task_data(task: task::TaskData) -> pb::TaskData {
    pb::TaskData {
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
        priority: task.priority.map(priority_tier_value),
    }
}

fn task_view(task: task::TaskView) -> pb::TaskView {
    pb::TaskView {
        id: task.id.to_string(),
        project: task.project.to_string(),
        status: task_status_value(task.status),
        heading: task.heading.to_string(),
        prompt: task.prompt.to_string(),
        project_path: task.project_path.to_string(),
        location: Some(pb::TaskLocation {
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
        priority: task.priority.map(priority_tier_value),
    }
}

fn task_issue(issue: &task::TaskIssue) -> pb::TaskIssue {
    match issue {
        task::TaskIssue::MissingNote { path } => pb::TaskIssue {
            kind: pb::TaskIssueKind::MissingNote as i32,
            path: Some(path.to_string()),
        },
        task::TaskIssue::PlaceholderPrompt => pb::TaskIssue {
            kind: pb::TaskIssueKind::PlaceholderPrompt as i32,
            path: None,
        },
    }
}

fn task_status_value(status: TaskStatus) -> i32 {
    match status {
        TaskStatus::Active => pb::TaskStatus::Active as i32,
        TaskStatus::Done => pb::TaskStatus::Done as i32,
        TaskStatus::Cancelled => pb::TaskStatus::Cancelled as i32,
    }
}

fn effort_tier_value(effort: EffortTier) -> i32 {
    match effort {
        EffortTier::Low => pb::EffortTier::Low as i32,
        EffortTier::Medium => pb::EffortTier::Medium as i32,
        EffortTier::High => pb::EffortTier::High as i32,
        EffortTier::Highest => pb::EffortTier::Highest as i32,
    }
}

fn priority_tier_value(priority: PriorityTier) -> i32 {
    match priority {
        PriorityTier::Low => pb::PriorityTier::Low as i32,
        PriorityTier::Medium => pb::PriorityTier::Medium as i32,
        PriorityTier::High => pb::PriorityTier::High as i32,
        PriorityTier::Highest => pb::PriorityTier::Highest as i32,
    }
}
