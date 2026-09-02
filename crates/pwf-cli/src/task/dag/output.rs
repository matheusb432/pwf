use mermaid_text::{Direction, Edge, Graph, Node, NodeShape};
use pwf_client::task::{TaskDag, TaskDagNode};
use pwf_models::task::TaskStatus;

use super::NodeFieldChoice;
use crate::task::render::render_domain_task_identifier;

pub(super) fn render(
    task_dag: TaskDag,
    node_field: Option<NodeFieldChoice>,
    color_on: bool,
) -> String {
    let PreparedTaskDag {
        graph,
        colored_task_labels,
    } = prepare(task_dag, node_field, color_on);

    let positions = mermaid_text::layout::layered::layout(
        &graph,
        &mermaid_text::layout::layered::LayoutConfig::default(),
    )
    .positions;
    let subgraph_bounds =
        mermaid_text::layout::subgraph::compute_subgraph_bounds(&graph, &positions);
    let mut output = mermaid_text::render::render(&graph, &positions, &subgraph_bounds);
    for (plain, colored) in colored_task_labels {
        output = output.replacen(&plain, &colored, 1);
    }
    output
}

pub(super) struct PreparedTaskDag {
    graph: Graph,
    colored_task_labels: Vec<(String, String)>,
}

pub(super) fn prepare(
    task_dag: TaskDag,
    node_field: Option<NodeFieldChoice>,
    color_on: bool,
) -> PreparedTaskDag {
    let (root_id, nodes, edges) = task_dag.into_parts();
    let task_dag_node_count = nodes.len();
    let mut graph = Graph::new(Direction::LeftToRight);
    graph.nodes.reserve(task_dag_node_count);
    graph.edges.reserve(edges.len());
    let mut colored_task_labels = if color_on {
        Vec::with_capacity(task_dag_node_count)
    } else {
        Vec::new()
    };
    for (node_index, node) in nodes.into_iter().enumerate() {
        let (label, shape) = match node {
            TaskDagNode::Task { id, title, status } => {
                let is_root = id == root_id;
                let identifier = id.into_string();
                let colored_identifier =
                    color_on.then(|| render_domain_task_identifier(&identifier, status, true));
                let label = task_label(identifier, status, &title, node_field);
                if let Some(colored_identifier) = colored_identifier {
                    let colored_label = task_label(colored_identifier, status, &title, node_field);
                    colored_task_labels.push((label.clone(), colored_label));
                }
                (
                    label,
                    if is_root {
                        NodeShape::Rounded
                    } else {
                        NodeShape::Rectangle
                    },
                )
            }
            TaskDagNode::Missing { id } => (
                annotated_label(id.into_string(), "missing"),
                NodeShape::Rectangle,
            ),
            TaskDagNode::Unavailable { id } => (
                annotated_label(id.into_string(), "unavailable"),
                NodeShape::Rectangle,
            ),
            TaskDagNode::DepthLimit => ("... [depth limit]".to_string(), NodeShape::Rectangle),
        };
        graph
            .nodes
            .push(Node::new(node_key(node_index), label, shape));
    }

    for edge in edges {
        graph.edges.push(Edge::new(
            node_key(edge.blocker_node_index),
            node_key(edge.dependent_node_index),
            None,
        ));
    }

    PreparedTaskDag {
        graph,
        colored_task_labels,
    }
}

fn task_label(
    identifier: String,
    status: TaskStatus,
    title: &str,
    node_field: Option<NodeFieldChoice>,
) -> String {
    match node_field {
        None => identifier,
        Some(NodeFieldChoice::Title) if title.is_empty() => identifier,
        Some(NodeFieldChoice::Title) => {
            let mut label = identifier;
            label.reserve(1 + title.len());
            label.push(' ');
            label.push_str(title);
            label
        }
        Some(NodeFieldChoice::Status) => annotated_label(identifier, status.as_str()),
    }
}

fn annotated_label(mut label: String, annotation: &str) -> String {
    label.reserve(3 + annotation.len());
    label.push_str(" [");
    label.push_str(annotation);
    label.push(']');
    label
}

fn node_key(node_index: impl std::fmt::Display) -> String {
    format!("node_{node_index}")
}

#[cfg(test)]
mod tests {
    use pwf_client::task::{TaskDag, TaskDagEdge, TaskDagNode};
    use pwf_models::task::{TaskId, TaskStatus};

    #[test]
    fn root_is_rounded_and_blocker_is_rectangular() {
        let output = super::render(
            task_dag(
                "FOO-0002",
                vec![
                    TaskDagNode::Task {
                        id: task_id("FOO-0002"),
                        title: "render graph view".to_string(),
                        status: TaskStatus::Active,
                    },
                    TaskDagNode::Task {
                        id: task_id("FOO-0001"),
                        title: "prepare graph data".to_string(),
                        status: TaskStatus::Done,
                    },
                ],
                vec![TaskDagEdge {
                    blocker_node_index: 1,
                    dependent_node_index: 0,
                }],
            ),
            Some(super::NodeFieldChoice::Status),
            false,
        );

        assert!(output.contains("FOO-0001 [done]"));
        assert!(output.contains("FOO-0002 [active]"));
        assert!(!output.contains("prepare graph data"));
        assert!(!output.contains("render graph view"));
        assert!(output.contains('┌'));
        assert!(output.contains('╭'));
        assert!(output.contains('▸'));
    }

    #[test]
    fn isolated_task_renders_without_an_empty_graph_message() {
        let output = super::render(
            task_dag(
                "FOO-0001",
                vec![TaskDagNode::Task {
                    id: task_id("FOO-0001"),
                    title: "standalone task".to_string(),
                    status: TaskStatus::Active,
                }],
                Vec::new(),
            ),
            None,
            false,
        );

        assert!(output.contains("FOO-0001"));
        assert!(!output.contains("[active]"));
        assert!(!output.contains("standalone task"));
        assert!(!output.to_ascii_lowercase().contains("no edges"));
    }

    #[test]
    fn missing_and_unavailable_nodes_explain_the_terminal_reference() {
        let output = super::render(
            task_dag(
                "FOO-0001",
                vec![
                    TaskDagNode::Task {
                        id: task_id("FOO-0001"),
                        title: "blocked root".to_string(),
                        status: TaskStatus::Active,
                    },
                    TaskDagNode::Missing {
                        id: task_id("FOO-0002"),
                    },
                    TaskDagNode::Unavailable {
                        id: task_id("AUX-0001"),
                    },
                ],
                vec![
                    TaskDagEdge {
                        blocker_node_index: 1,
                        dependent_node_index: 0,
                    },
                    TaskDagEdge {
                        blocker_node_index: 2,
                        dependent_node_index: 0,
                    },
                ],
            ),
            None,
            false,
        );

        assert!(output.contains("FOO-0002 [missing]"));
        assert!(output.contains("AUX-0001 [unavailable]"));
    }

    #[test]
    fn task_identifiers_use_lifecycle_colors_without_coloring_extra_fields() {
        let output = super::render(
            task_dag(
                "FOO-0001",
                vec![
                    task_node("FOO-0001", TaskStatus::Active),
                    task_node("FOO-0002", TaskStatus::Done),
                    task_node("FOO-0003", TaskStatus::Cancelled),
                ],
                vec![
                    TaskDagEdge {
                        blocker_node_index: 1,
                        dependent_node_index: 0,
                    },
                    TaskDagEdge {
                        blocker_node_index: 2,
                        dependent_node_index: 0,
                    },
                ],
            ),
            Some(super::NodeFieldChoice::Status),
            true,
        );

        assert!(output.contains("\u{1b}[34mFOO-0001\u{1b}[0m [active]"));
        assert!(output.contains("\u{1b}[32mFOO-0002\u{1b}[0m [done]"));
        assert!(output.contains("\u{1b}[31mFOO-0003\u{1b}[0m [cancelled]"));
    }

    fn task_node(id: &str, status: TaskStatus) -> TaskDagNode {
        TaskDagNode::Task {
            id: task_id(id),
            title: "unused title".to_string(),
            status,
        }
    }

    fn task_dag(root_id: &str, nodes: Vec<TaskDagNode>, edges: Vec<TaskDagEdge>) -> TaskDag {
        TaskDag::try_new(task_id(root_id), nodes, edges).unwrap()
    }

    fn task_id(value: &str) -> TaskId {
        TaskId::try_new(value).unwrap()
    }
}
