use std::{collections::BTreeSet, num::NonZeroU32};

use pwf_models::task::{TaskId, TaskStatus};

use super::StatusFilter;

/// Requests one bounded dependency graph rooted at a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetTaskDag {
    pub id: TaskId,
    pub depth: Option<TaskDagDepth>,
    pub status: StatusFilter,
    pub mode: TaskDagMode,
}

/// Selects the relationship direction traversed by a task DAG query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskDagMode {
    BlockedBy,
    Blocks,
    Full,
}

impl TaskDagMode {
    #[must_use]
    pub const fn includes_blocked_by(self) -> bool {
        matches!(self, Self::BlockedBy | Self::Full)
    }

    #[must_use]
    pub const fn includes_blocks(self) -> bool {
        matches!(self, Self::Blocks | Self::Full)
    }
}

/// Caps graph traversal by the number of edges from the selected task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskDagDepth(NonZeroU32);

impl TaskDagDepth {
    /// Constructs a nonzero traversal depth.
    ///
    /// # Errors
    ///
    /// Returns [`TaskDagDepthError`] when `value` is zero.
    pub fn try_new(value: u32) -> Result<Self, TaskDagDepthError> {
        NonZeroU32::new(value).map(Self).ok_or(TaskDagDepthError)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

/// Reports a zero task-DAG traversal depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("depth must be at least 1")]
pub struct TaskDagDepthError;

/// Carries a bounded task dependency graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDag {
    root_id: TaskId,
    nodes: Vec<TaskDagNode>,
    edges: Vec<TaskDagEdge>,
}

impl TaskDag {
    pub const NODE_COUNT_MAX: usize = 512;
    pub const EDGE_COUNT_MAX: usize = 2_048;

    /// Constructs a bounded, rooted acyclic task graph.
    ///
    /// # Errors
    ///
    /// Returns [`TaskDagError`] when the root, nodes, or edges do not form a valid task DAG.
    pub fn try_new(
        root_id: TaskId,
        nodes: Vec<TaskDagNode>,
        edges: Vec<TaskDagEdge>,
    ) -> Result<Self, TaskDagError> {
        if nodes.len() > Self::NODE_COUNT_MAX {
            return Err(TaskDagError::NodeLimit {
                max: Self::NODE_COUNT_MAX,
            });
        }
        if edges.len() > Self::EDGE_COUNT_MAX {
            return Err(TaskDagError::EdgeLimit {
                max: Self::EDGE_COUNT_MAX,
            });
        }

        let root_node_index = validate_task_dag_nodes(&root_id, &nodes)?;
        validate_task_dag_edges(root_node_index, nodes.len(), &edges)?;

        Ok(Self {
            root_id,
            nodes,
            edges,
        })
    }

    #[must_use]
    pub fn root_id(&self) -> &TaskId {
        &self.root_id
    }

    #[must_use]
    pub fn nodes(&self) -> &[TaskDagNode] {
        &self.nodes
    }

    #[must_use]
    pub fn edges(&self) -> &[TaskDagEdge] {
        &self.edges
    }

    #[must_use]
    pub fn into_parts(self) -> (TaskId, Vec<TaskDagNode>, Vec<TaskDagEdge>) {
        (self.root_id, self.nodes, self.edges)
    }
}

/// Reports values that do not form a bounded, rooted acyclic task graph.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TaskDagError {
    #[error("task dependency graph exceeds the {max} node limit")]
    NodeLimit { max: usize },
    #[error("task dependency graph exceeds the {max} edge limit")]
    EdgeLimit { max: usize },
    #[error("task dependency graph root {id} is not a task node")]
    RootTaskMissing { id: TaskId },
    #[error("task dependency graph repeats task node {id}")]
    DuplicateTaskNode { id: TaskId },
    #[error(
        "task dependency graph edge {edge_index} references blocker node {node_index}, but the graph contains {node_count} nodes"
    )]
    BlockerNodeIndex {
        edge_index: usize,
        node_index: usize,
        node_count: usize,
    },
    #[error(
        "task dependency graph edge {edge_index} references dependent node {node_index}, but the graph contains {node_count} nodes"
    )]
    DependentNodeIndex {
        edge_index: usize,
        node_index: usize,
        node_count: usize,
    },
    #[error("task dependency graph repeats edge {edge:?}")]
    DuplicateEdge { edge: TaskDagEdge },
    #[error("task dependency graph contains a cycle")]
    Cycle,
    #[error("task dependency graph node {node_index} is disconnected from the root")]
    DisconnectedNode { node_index: usize },
}

fn validate_task_dag_nodes(root_id: &TaskId, nodes: &[TaskDagNode]) -> Result<usize, TaskDagError> {
    let mut task_ids = BTreeSet::new();
    let mut root_node_index = None;
    for (node_index, node) in nodes.iter().enumerate() {
        let Some(id) = node.task_id() else {
            continue;
        };
        if !task_ids.insert(id) {
            return Err(TaskDagError::DuplicateTaskNode { id: id.clone() });
        }
        if matches!(node, TaskDagNode::Task { .. }) && id == root_id {
            root_node_index = Some(node_index);
        }
    }
    root_node_index.ok_or_else(|| TaskDagError::RootTaskMissing {
        id: root_id.clone(),
    })
}

fn validate_task_dag_edges(
    root_node_index: usize,
    node_count: usize,
    edges: &[TaskDagEdge],
) -> Result<(), TaskDagError> {
    let mut unique_edges = BTreeSet::new();
    let mut dependent_counts = vec![0_usize; node_count];
    let mut dependents = vec![Vec::new(); node_count];
    let mut adjacent_nodes = vec![Vec::new(); node_count];

    for (edge_index, edge) in edges.iter().copied().enumerate() {
        if edge.blocker_node_index >= node_count {
            return Err(TaskDagError::BlockerNodeIndex {
                edge_index,
                node_index: edge.blocker_node_index,
                node_count,
            });
        }
        if edge.dependent_node_index >= node_count {
            return Err(TaskDagError::DependentNodeIndex {
                edge_index,
                node_index: edge.dependent_node_index,
                node_count,
            });
        }
        if !unique_edges.insert(edge) {
            return Err(TaskDagError::DuplicateEdge { edge });
        }

        dependent_counts[edge.dependent_node_index] += 1;
        dependents[edge.blocker_node_index].push(edge.dependent_node_index);
        adjacent_nodes[edge.blocker_node_index].push(edge.dependent_node_index);
        adjacent_nodes[edge.dependent_node_index].push(edge.blocker_node_index);
    }

    validate_task_dag_acyclic(&dependents, dependent_counts)?;
    validate_task_dag_connected(root_node_index, &adjacent_nodes)
}

fn validate_task_dag_acyclic(
    dependents: &[Vec<usize>],
    mut dependent_counts: Vec<usize>,
) -> Result<(), TaskDagError> {
    let mut unblocked_nodes = dependent_counts
        .iter()
        .enumerate()
        .filter_map(|(node_index, count)| (*count == 0).then_some(node_index))
        .collect::<Vec<_>>();
    let mut visited_node_count = 0;
    while let Some(node_index) = unblocked_nodes.pop() {
        visited_node_count += 1;
        for dependent_node_index in &dependents[node_index] {
            dependent_counts[*dependent_node_index] -= 1;
            if dependent_counts[*dependent_node_index] == 0 {
                unblocked_nodes.push(*dependent_node_index);
            }
        }
    }
    if visited_node_count == dependents.len() {
        Ok(())
    } else {
        Err(TaskDagError::Cycle)
    }
}

fn validate_task_dag_connected(
    root_node_index: usize,
    adjacent_nodes: &[Vec<usize>],
) -> Result<(), TaskDagError> {
    let mut visited_nodes = vec![false; adjacent_nodes.len()];
    let mut pending_nodes = vec![root_node_index];
    while let Some(node_index) = pending_nodes.pop() {
        if visited_nodes[node_index] {
            continue;
        }
        visited_nodes[node_index] = true;
        pending_nodes.extend(adjacent_nodes[node_index].iter().copied());
    }
    match visited_nodes.iter().position(|visited| !visited) {
        Some(node_index) => Err(TaskDagError::DisconnectedNode { node_index }),
        None => Ok(()),
    }
}

/// Describes one task or synthetic truncation point in a dependency graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskDagNode {
    Task {
        id: TaskId,
        title: String,
        status: TaskStatus,
    },
    Missing {
        id: TaskId,
    },
    Unavailable {
        id: TaskId,
    },
    DepthLimit,
}

impl TaskDagNode {
    #[must_use]
    pub fn task_id(&self) -> Option<&TaskId> {
        match self {
            Self::Task { id, .. } | Self::Missing { id } | Self::Unavailable { id } => Some(id),
            Self::DepthLimit => None,
        }
    }
}

/// Connects two node indexes in blocker-to-dependent direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskDagEdge {
    pub blocker_node_index: usize,
    pub dependent_node_index: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_dag_rejects_a_root_without_a_task_node() {
        let root_id = task_id("PWF-0001");

        let error = TaskDag::try_new(
            root_id.clone(),
            vec![TaskDagNode::Missing {
                id: root_id.clone(),
            }],
            Vec::new(),
        )
        .unwrap_err();

        assert_eq!(error, TaskDagError::RootTaskMissing { id: root_id });
    }

    #[test]
    fn task_dag_rejects_collection_limits() {
        let root_id = task_id("PWF-0001");
        let node_limit_error = TaskDag::try_new(
            root_id.clone(),
            vec![TaskDagNode::DepthLimit; TaskDag::NODE_COUNT_MAX + 1],
            Vec::new(),
        )
        .unwrap_err();
        let edge_limit_error = TaskDag::try_new(
            root_id.clone(),
            vec![task_dag_node(&root_id)],
            vec![
                TaskDagEdge {
                    blocker_node_index: 0,
                    dependent_node_index: 0,
                };
                TaskDag::EDGE_COUNT_MAX + 1
            ],
        )
        .unwrap_err();

        assert_eq!(
            node_limit_error,
            TaskDagError::NodeLimit {
                max: TaskDag::NODE_COUNT_MAX,
            }
        );
        assert_eq!(
            edge_limit_error,
            TaskDagError::EdgeLimit {
                max: TaskDag::EDGE_COUNT_MAX,
            }
        );
    }

    #[test]
    fn task_dag_rejects_duplicate_task_nodes() {
        let root_id = task_id("PWF-0001");

        let error = TaskDag::try_new(
            root_id.clone(),
            vec![task_dag_node(&root_id), task_dag_node(&root_id)],
            Vec::new(),
        )
        .unwrap_err();

        assert_eq!(error, TaskDagError::DuplicateTaskNode { id: root_id });
    }

    #[test]
    fn task_dag_rejects_an_edge_outside_its_node_collection() {
        let root_id = task_id("PWF-0001");

        let error = TaskDag::try_new(
            root_id.clone(),
            vec![task_dag_node(&root_id)],
            vec![TaskDagEdge {
                blocker_node_index: 1,
                dependent_node_index: 0,
            }],
        )
        .unwrap_err();

        assert_eq!(
            error,
            TaskDagError::BlockerNodeIndex {
                edge_index: 0,
                node_index: 1,
                node_count: 1,
            }
        );
    }

    #[test]
    fn task_dag_rejects_duplicate_edges() {
        let root_id = task_id("PWF-0001");
        let blocker_id = task_id("PWF-0002");
        let edge = TaskDagEdge {
            blocker_node_index: 1,
            dependent_node_index: 0,
        };

        let error = TaskDag::try_new(
            root_id.clone(),
            vec![task_dag_node(&root_id), task_dag_node(&blocker_id)],
            vec![edge, edge],
        )
        .unwrap_err();

        assert_eq!(error, TaskDagError::DuplicateEdge { edge });
    }

    #[test]
    fn task_dag_rejects_cycles() {
        let root_id = task_id("PWF-0001");
        let blocker_id = task_id("PWF-0002");

        let error = TaskDag::try_new(
            root_id.clone(),
            vec![task_dag_node(&root_id), task_dag_node(&blocker_id)],
            vec![
                TaskDagEdge {
                    blocker_node_index: 1,
                    dependent_node_index: 0,
                },
                TaskDagEdge {
                    blocker_node_index: 0,
                    dependent_node_index: 1,
                },
            ],
        )
        .unwrap_err();

        assert_eq!(error, TaskDagError::Cycle);
    }

    #[test]
    fn task_dag_rejects_nodes_disconnected_from_the_root() {
        let root_id = task_id("PWF-0001");
        let unrelated_id = task_id("PWF-0002");

        let error = TaskDag::try_new(
            root_id.clone(),
            vec![task_dag_node(&root_id), task_dag_node(&unrelated_id)],
            Vec::new(),
        )
        .unwrap_err();

        assert_eq!(error, TaskDagError::DisconnectedNode { node_index: 1 });
    }

    fn task_dag_node(id: &TaskId) -> TaskDagNode {
        TaskDagNode::Task {
            id: id.clone(),
            title: String::new(),
            status: TaskStatus::Active,
        }
    }

    fn task_id(value: &str) -> TaskId {
        TaskId::try_new(value).unwrap()
    }
}
