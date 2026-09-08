//! Protobuf response mappings for task operations.

use pwf_models::task::{EffortTier, PriorityTier, TaskId, TaskIdError, TaskStatus};

use crate::{confirmation, pb, task};

#[must_use]
pub fn create_task_response(result: task::TaskMutationResult<TaskId>) -> pb::CreateTaskResponse {
    pb::CreateTaskResponse {
        id: result.outcome.to_string(),
        task: result.task.map(task_mutation_summary),
    }
}

#[must_use]
pub fn update_task_response(result: task::TaskMutationResult<()>) -> pb::UpdateTaskResponse {
    pb::UpdateTaskResponse {
        task: result.task.map(task_mutation_summary),
    }
}

#[must_use]
pub fn cancel_task_response(result: task::TaskMutationResult<()>) -> pb::CancelTaskResponse {
    pb::CancelTaskResponse {
        task: result.task.map(task_mutation_summary),
    }
}

#[must_use]
pub fn complete_task_response(result: task::TaskMutationResult<()>) -> pb::CompleteTaskResponse {
    pb::CompleteTaskResponse {
        task: result.task.map(task_mutation_summary),
    }
}

fn task_mutation_summary(task: task::TaskMutationSummary) -> pb::TaskMutationSummary {
    pb::TaskMutationSummary {
        id: task.id.to_string(),
        title: task.title,
        status: match task.status {
            TaskStatus::Active => pb::TaskStatus::Active,
            TaskStatus::Done => pb::TaskStatus::Done,
            TaskStatus::Cancelled => pb::TaskStatus::Cancelled,
        } as i32,
    }
}

#[must_use]
pub fn reopen_task_result(
    result: task::TaskMutationResult<task::ReopenTaskOutcome>,
) -> pb::ReopenTaskResult {
    let task = result.task.map(task_mutation_summary);
    let outcome = match result.outcome {
        task::ReopenTaskOutcome::Reopened => {
            pb::reopen_task_result::Outcome::Reopened(pb::ReopenedTask { task })
        }
        task::ReopenTaskOutcome::AlreadyActive => {
            pb::reopen_task_result::Outcome::AlreadyActive(pb::AlreadyActiveTask { task })
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
pub fn delete_task_result(
    result: task::TaskMutationResult<task::DeleteTaskOutcome>,
) -> pb::DeleteTaskResult {
    let outcome = match result.outcome {
        task::DeleteTaskOutcome::Deleted => {
            pb::delete_task_result::Outcome::Deleted(pb::DeletedTask {
                task: result.task.map(task_mutation_summary),
            })
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
#[must_use]
pub fn get_task_dag_response(graph: task::TaskDag) -> pb::GetTaskDagResponse {
    let (root_id, nodes, edges) = graph.into_parts();
    pb::GetTaskDagResponse {
        root_id: root_id.to_string(),
        nodes: nodes.into_iter().map(task_dag_node).collect(),
        edges: edges
            .into_iter()
            .map(|edge| pb::TaskDagEdge {
                blocker_node_index: task_dag_node_index(edge.blocker_node_index),
                dependent_node_index: task_dag_node_index(edge.dependent_node_index),
            })
            .collect(),
    }
}

#[allow(clippy::cast_possible_truncation)]
fn task_dag_node_index(node_index: usize) -> u32 {
    // TaskDag construction bounds every edge index below its 512-node cap.
    node_index as u32
}

/// Decodes a Protobuf task-DAG response into its native representation.
///
/// # Errors
///
/// Returns [`DecodeGetTaskDagResponseError`] when a wire value cannot represent
/// a native task ID, lifecycle status, node, or node index.
pub fn decode_get_task_dag_response(
    response: pb::GetTaskDagResponse,
) -> Result<task::TaskDag, DecodeGetTaskDagResponseError> {
    let root_id =
        TaskId::try_new(response.root_id).map_err(DecodeGetTaskDagResponseError::RootTaskId)?;
    let nodes = response
        .nodes
        .into_iter()
        .enumerate()
        .map(|(node_index, node)| decode_task_dag_node(node_index, node))
        .collect::<Result<Vec<_>, DecodeGetTaskDagResponseError>>()?;
    let edges = response
        .edges
        .into_iter()
        .enumerate()
        .map(|(edge_index, edge)| {
            Ok(task::TaskDagEdge {
                blocker_node_index: usize::try_from(edge.blocker_node_index).map_err(|source| {
                    DecodeGetTaskDagResponseError::BlockerNodeIndex { edge_index, source }
                })?,
                dependent_node_index: usize::try_from(edge.dependent_node_index).map_err(
                    |source| DecodeGetTaskDagResponseError::DependentNodeIndex {
                        edge_index,
                        source,
                    },
                )?,
            })
        })
        .collect::<Result<Vec<_>, DecodeGetTaskDagResponseError>>()?;
    task::TaskDag::try_new(root_id, nodes, edges).map_err(Into::into)
}

/// Reports a task-DAG wire value that cannot be represented natively.
#[derive(Debug, thiserror::Error)]
pub enum DecodeGetTaskDagResponseError {
    #[error("task dependency graph has an invalid root task ID")]
    RootTaskId(#[source] TaskIdError),
    #[error("task dependency graph node {node_index} is empty")]
    EmptyNode { node_index: usize },
    #[error("task dependency graph node {node_index} has an invalid task ID")]
    NodeTaskId {
        node_index: usize,
        #[source]
        source: TaskIdError,
    },
    #[error("task dependency graph node {node_index} has invalid lifecycle status {value}")]
    NodeTaskStatus { node_index: usize, value: i32 },
    #[error("task dependency graph edge {edge_index} has an out-of-range blocker node index")]
    BlockerNodeIndex {
        edge_index: usize,
        #[source]
        source: std::num::TryFromIntError,
    },
    #[error("task dependency graph edge {edge_index} has an out-of-range dependent node index")]
    DependentNodeIndex {
        edge_index: usize,
        #[source]
        source: std::num::TryFromIntError,
    },
    #[error(transparent)]
    TaskDag(#[from] task::TaskDagError),
}

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

fn decode_task_dag_node(
    node_index: usize,
    node: pb::TaskDagNode,
) -> Result<task::TaskDagNode, DecodeGetTaskDagResponseError> {
    let value = node
        .value
        .ok_or(DecodeGetTaskDagResponseError::EmptyNode { node_index })?;
    match value {
        pb::task_dag_node::Value::Task(node) => Ok(task::TaskDagNode::Task {
            id: decode_task_dag_node_id(node_index, node.id)?,
            title: node.title,
            status: decode_task_dag_status(node_index, node.status)?,
        }),
        pb::task_dag_node::Value::Missing(node) => Ok(task::TaskDagNode::Missing {
            id: decode_task_dag_node_id(node_index, node.id)?,
        }),
        pb::task_dag_node::Value::Unavailable(node) => Ok(task::TaskDagNode::Unavailable {
            id: decode_task_dag_node_id(node_index, node.id)?,
        }),
        pb::task_dag_node::Value::DepthLimit(_) => Ok(task::TaskDagNode::DepthLimit),
    }
}

fn decode_task_dag_node_id(
    node_index: usize,
    id: String,
) -> Result<TaskId, DecodeGetTaskDagResponseError> {
    TaskId::try_new(id)
        .map_err(|source| DecodeGetTaskDagResponseError::NodeTaskId { node_index, source })
}

fn decode_task_dag_status(
    node_index: usize,
    value: i32,
) -> Result<TaskStatus, DecodeGetTaskDagResponseError> {
    match pb::TaskStatus::try_from(value) {
        Ok(pb::TaskStatus::Active) => Ok(TaskStatus::Active),
        Ok(pb::TaskStatus::Done) => Ok(TaskStatus::Done),
        Ok(pb::TaskStatus::Cancelled) => Ok(TaskStatus::Cancelled),
        Ok(pb::TaskStatus::Unspecified) | Err(_) => {
            Err(DecodeGetTaskDagResponseError::NodeTaskStatus { node_index, value })
        }
    }
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
        obsidian_vault: match &confirmation.deletion {
            crate::confirmation::TaskDeletion::HardDelete => None,
            crate::confirmation::TaskDeletion::MoveToTrash { obsidian_vault } => {
                Some(obsidian_vault.to_string_lossy().into_owned())
            }
        },
        trash_folder: confirmation
            .deletion
            .trash_folder()
            .map(|path| path.to_string_lossy().into_owned()),
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

fn task_view(task: task::ListedTask) -> pb::TaskView {
    let mut view = pb::TaskView {
        id: task.id.to_string(),
        project: task.project.to_string(),
        status: task_status_value(task.status),
        heading: task.heading.to_string(),
        section: task.section.map(|section| section.to_string()),
        effort: task.effort.map(effort_tier_value),
        raw_tags: task.tags.map(|tags| tags.to_string()),
        created: task.created.map(|date| date.to_string()),
        priority: task.priority.map(priority_tier_value),
        ..Default::default()
    };
    if let Some(details) = task.details {
        view.prompt = details.prompt.to_string();
        view.project_path = details.project_path.map(|path| path.to_string());
        view.location = Some(pb::TaskLocation {
            index_path: details.location.index_path().to_string(),
            line: details.location.line().get() as u64,
        });
        view.launch_issues = details.launch.issues().iter().map(task_issue).collect();
        view.blocked_by = details
            .blocked_by
            .map(|blockers| blockers.iter().map(ToString::to_string).collect())
            .unwrap_or_default();
        view.blocked_by_statuses = details
            .blocked_by_statuses
            .into_iter()
            .map(blocked_by_status)
            .collect();
        view.blocked_by_issues = details
            .blocked_by_issues
            .into_iter()
            .map(blocked_by_issue)
            .collect();
    }
    view
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

#[cfg(test)]
mod tests {
    use pwf_models::task::{TaskId, TaskStatus};

    use super::{DecodeGetTaskDagResponseError, decode_get_task_dag_response};
    use crate::{pb, task};

    #[test]
    fn task_dag_response_decodes_to_native_values() {
        let task_dag = decode_get_task_dag_response(pb::GetTaskDagResponse {
            root_id: "PWF-0001".to_string(),
            nodes: vec![
                task_node("PWF-0001", pb::TaskStatus::Active),
                node(pb::task_dag_node::Value::Missing(pb::TaskDagMissingNode {
                    id: "PWF-0002".to_string(),
                })),
                node(pb::task_dag_node::Value::Unavailable(
                    pb::TaskDagUnavailableNode {
                        id: "AUX-0001".to_string(),
                    },
                )),
                node(pb::task_dag_node::Value::DepthLimit(
                    pb::TaskDagDepthLimitNode {},
                )),
            ],
            edges: vec![
                pb::TaskDagEdge {
                    blocker_node_index: 1,
                    dependent_node_index: 0,
                },
                pb::TaskDagEdge {
                    blocker_node_index: 2,
                    dependent_node_index: 0,
                },
                pb::TaskDagEdge {
                    blocker_node_index: 3,
                    dependent_node_index: 0,
                },
            ],
        })
        .unwrap();

        assert_eq!(
            task_dag,
            task::TaskDag::try_new(
                task_id("PWF-0001"),
                vec![
                    task::TaskDagNode::Task {
                        id: task_id("PWF-0001"),
                        title: "task PWF-0001".to_string(),
                        status: TaskStatus::Active,
                    },
                    task::TaskDagNode::Missing {
                        id: task_id("PWF-0002"),
                    },
                    task::TaskDagNode::Unavailable {
                        id: task_id("AUX-0001"),
                    },
                    task::TaskDagNode::DepthLimit,
                ],
                vec![
                    task::TaskDagEdge {
                        blocker_node_index: 1,
                        dependent_node_index: 0,
                    },
                    task::TaskDagEdge {
                        blocker_node_index: 2,
                        dependent_node_index: 0,
                    },
                    task::TaskDagEdge {
                        blocker_node_index: 3,
                        dependent_node_index: 0,
                    },
                ],
            )
            .unwrap()
        );
    }

    #[test]
    fn task_dag_response_rejects_an_empty_node() {
        let error = decode_get_task_dag_response(pb::GetTaskDagResponse {
            root_id: "PWF-0001".to_string(),
            nodes: vec![pb::TaskDagNode { value: None }],
            edges: Vec::new(),
        })
        .unwrap_err();

        assert!(matches!(
            error,
            DecodeGetTaskDagResponseError::EmptyNode { node_index: 0 }
        ));
    }

    #[test]
    fn task_dag_response_rejects_an_invalid_task_id() {
        let error = decode_get_task_dag_response(pb::GetTaskDagResponse {
            root_id: "PWF-0001".to_string(),
            nodes: vec![task_node("invalid", pb::TaskStatus::Active)],
            edges: Vec::new(),
        })
        .unwrap_err();

        assert!(matches!(
            error,
            DecodeGetTaskDagResponseError::NodeTaskId { node_index: 0, .. }
        ));
    }

    #[test]
    fn task_dag_response_rejects_an_invalid_task_status() {
        let error = decode_get_task_dag_response(pb::GetTaskDagResponse {
            root_id: "PWF-0001".to_string(),
            nodes: vec![task_node("PWF-0001", pb::TaskStatus::Unspecified)],
            edges: Vec::new(),
        })
        .unwrap_err();

        assert!(matches!(
            error,
            DecodeGetTaskDagResponseError::NodeTaskStatus {
                node_index: 0,
                value: 0,
            }
        ));
    }

    fn node(value: pb::task_dag_node::Value) -> pb::TaskDagNode {
        pb::TaskDagNode { value: Some(value) }
    }

    fn task_node(id: &str, status: pb::TaskStatus) -> pb::TaskDagNode {
        node(pb::task_dag_node::Value::Task(pb::TaskDagTaskNode {
            id: id.to_string(),
            title: format!("task {id}"),
            status: status as i32,
        }))
    }

    fn task_id(value: &str) -> TaskId {
        TaskId::try_new(value).unwrap()
    }
}
