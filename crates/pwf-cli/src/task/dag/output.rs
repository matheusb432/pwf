use mermaid_text::{Direction, Edge, Graph, Node, NodeShape};
use pwf_client::task::{TaskDag, TaskDagNode};
use pwf_models::{settings::TaskStatusColors, task::TaskStatus};

use super::NodeFieldChoice;
use crate::task::render::render_domain_task_identifier;

pub(super) fn render(
    task_dag: TaskDag,
    node_field: Option<NodeFieldChoice>,
    task_status_colors: TaskStatusColors,
    color_on: bool,
) -> String {
    if color_on {
        render_colored(task_dag, node_field, task_status_colors)
    } else {
        render_plain(task_dag, node_field)
    }
}

fn render_plain(task_dag: TaskDag, node_field: Option<NodeFieldChoice>) -> String {
    render_graph(&prepare(task_dag, node_field))
}

fn render_colored(
    task_dag: TaskDag,
    node_field: Option<NodeFieldChoice>,
    task_status_colors: TaskStatusColors,
) -> String {
    let (graph, coloring) = prepare_colored(task_dag, node_field, task_status_colors);
    coloring.apply(render_graph(&graph))
}

fn render_graph(graph: &Graph) -> String {
    let positions = mermaid_text::layout::layered::layout(
        graph,
        &mermaid_text::layout::layered::LayoutConfig::default(),
    )
    .positions;
    let subgraph_bounds =
        mermaid_text::layout::subgraph::compute_subgraph_bounds(graph, &positions);
    mermaid_text::render::render(graph, &positions, &subgraph_bounds)
}

pub(super) fn prepare_colored(
    task_dag: TaskDag,
    node_field: Option<NodeFieldChoice>,
    task_status_colors: TaskStatusColors,
) -> (Graph, TaskDagColoring) {
    let coloring = TaskDagColoring::prepare(&task_dag, node_field, task_status_colors);
    let graph = prepare(task_dag, node_field);
    (graph, coloring)
}

pub(super) fn prepare(task_dag: TaskDag, node_field: Option<NodeFieldChoice>) -> Graph {
    let (root_id, nodes, edges) = task_dag.into_parts();
    let task_dag_node_count = nodes.len();
    let mut graph = Graph::new(Direction::LeftToRight);
    graph.nodes.reserve(task_dag_node_count);
    graph.edges.reserve(edges.len());
    for (node_index, node) in nodes.into_iter().enumerate() {
        let (label, shape) = match node {
            TaskDagNode::Task { id, title, status } => {
                let is_root = id == root_id;
                let identifier = id.into_string();
                let label = task_label(identifier, status, &title, node_field);
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

    graph
}

pub(super) struct TaskDagColoring {
    task_labels: Vec<(String, String)>,
}

impl TaskDagColoring {
    fn prepare(
        task_dag: &TaskDag,
        node_field: Option<NodeFieldChoice>,
        task_status_colors: TaskStatusColors,
    ) -> Self {
        let mut task_labels = Vec::with_capacity(task_dag.nodes().len());
        for node in task_dag.nodes() {
            let TaskDagNode::Task { id, title, status } = node else {
                continue;
            };
            let plain_label = task_label(id.to_string(), *status, title, node_field);
            let colored_identifier =
                render_domain_task_identifier(id.as_ref(), *status, task_status_colors, true);
            let colored_label = task_label(colored_identifier, *status, title, node_field);
            task_labels.push((plain_label, colored_label));
        }
        Self { task_labels }
    }

    fn apply(self, mut output: String) -> String {
        for (plain_label, colored_label) in self.task_labels {
            output = output.replacen(&plain_label, &colored_label, 1);
        }
        output
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
    use pwf_models::{
        settings::{RgbColor, TaskStatusColors},
        task::{TaskId, TaskStatus},
    };

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
            TaskStatusColors::default(),
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
            TaskStatusColors::default(),
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
            TaskStatusColors::default(),
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
            TaskStatusColors::default(),
            true,
        );

        assert!(output.contains("\u{1b}[34mFOO-0001\u{1b}[0m [active]"));
        assert!(output.contains("\u{1b}[32mFOO-0002\u{1b}[0m [done]"));
        assert!(output.contains("\u{1b}[31mFOO-0003\u{1b}[0m [cancelled]"));
    }

    #[test]
    fn task_identifiers_use_configured_lifecycle_colors() {
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
            TaskStatusColors::new(
                Some(RgbColor::new(1, 2, 3)),
                Some(RgbColor::new(4, 5, 6)),
                Some(RgbColor::new(7, 8, 9)),
            ),
            true,
        );

        assert!(output.contains("\u{1b}[38;2;1;2;3mFOO-0001\u{1b}[0m [active]"));
        assert!(output.contains("\u{1b}[38;2;4;5;6mFOO-0002\u{1b}[0m [done]"));
        assert!(output.contains("\u{1b}[38;2;7;8;9mFOO-0003\u{1b}[0m [cancelled]"));
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
