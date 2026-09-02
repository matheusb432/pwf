use anyhow::Context as _;
use mermaid_text::{Direction, Edge, Graph, Node, NodeShape};
use pwf_client::pb::{self, GetTaskDagResponse, TaskStatus};

use super::NodeFieldChoice;
use crate::task::render::render_task_identifier;

pub(super) fn render(
    response: &GetTaskDagResponse,
    node_field: Option<NodeFieldChoice>,
    color_on: bool,
) -> anyhow::Result<String> {
    let mut graph = Graph::new(Direction::LeftToRight);
    let mut colored_task_labels = Vec::new();
    for (node_index, node) in response.nodes.iter().enumerate() {
        let value = node
            .value
            .as_ref()
            .with_context(|| format!("pwf-server task dependency node {node_index} is empty"))?;
        let (label, shape) = match value {
            pb::task_dag_node::Value::Task(task) => {
                let status = TaskStatus::try_from(task.status).with_context(|| {
                    format!("pwf-server task dependency node {node_index} has invalid status")
                })?;
                let is_root = task.id == response.root_id;
                let label = task_label(&task.id, status, &task.title, node_field);
                if color_on {
                    let colored_identifier = render_task_identifier(&task.id, status, true);
                    let colored_label =
                        task_label(&colored_identifier, status, &task.title, node_field);
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
            pb::task_dag_node::Value::Missing(task) => {
                (format!("{} [missing]", task.id), NodeShape::Rectangle)
            }
            pb::task_dag_node::Value::Unavailable(task) => {
                (format!("{} [unavailable]", task.id), NodeShape::Rectangle)
            }
            pb::task_dag_node::Value::DepthLimit(_) => {
                ("... [depth limit]".to_string(), NodeShape::Rectangle)
            }
        };
        graph
            .nodes
            .push(Node::new(node_key(node_index), label, shape));
    }

    for edge in &response.edges {
        graph.edges.push(Edge::new(
            node_key(edge.blocker_node_index),
            node_key(edge.dependent_node_index),
            None,
        ));
    }

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
    Ok(output)
}

fn task_label(
    id: &str,
    status: TaskStatus,
    title: &str,
    node_field: Option<NodeFieldChoice>,
) -> String {
    match node_field {
        None => id.to_string(),
        Some(NodeFieldChoice::Title) if title.is_empty() => id.to_string(),
        Some(NodeFieldChoice::Title) => format!("{id} {title}"),
        Some(NodeFieldChoice::Status) => format!("{id} [{}]", task_status_name(status)),
    }
}

fn task_status_name(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Active => "active",
        TaskStatus::Done => "done",
        TaskStatus::Cancelled => "cancelled",
        TaskStatus::Unspecified => "unspecified",
    }
}

fn node_key(node_index: impl std::fmt::Display) -> String {
    format!("node_{node_index}")
}

#[cfg(test)]
mod tests {
    use pwf_client::pb::{
        GetTaskDagResponse, TaskDagEdge, TaskDagNode, TaskDagTaskNode, TaskStatus, task_dag_node,
    };

    #[test]
    fn root_is_rounded_and_blocker_is_rectangular() {
        let output = super::render(
            &GetTaskDagResponse {
                root_id: "FOO-0002".to_string(),
                nodes: vec![
                    TaskDagNode {
                        value: Some(task_dag_node::Value::Task(TaskDagTaskNode {
                            id: "FOO-0002".to_string(),
                            title: "render graph view".to_string(),
                            status: TaskStatus::Active as i32,
                        })),
                    },
                    TaskDagNode {
                        value: Some(task_dag_node::Value::Task(TaskDagTaskNode {
                            id: "FOO-0001".to_string(),
                            title: "prepare graph data".to_string(),
                            status: TaskStatus::Done as i32,
                        })),
                    },
                ],
                edges: vec![TaskDagEdge {
                    blocker_node_index: 1,
                    dependent_node_index: 0,
                }],
            },
            Some(super::NodeFieldChoice::Status),
            false,
        )
        .unwrap();

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
            &GetTaskDagResponse {
                root_id: "FOO-0001".to_string(),
                nodes: vec![TaskDagNode {
                    value: Some(task_dag_node::Value::Task(TaskDagTaskNode {
                        id: "FOO-0001".to_string(),
                        title: "standalone task".to_string(),
                        status: TaskStatus::Active as i32,
                    })),
                }],
                edges: Vec::new(),
            },
            None,
            false,
        )
        .unwrap();

        assert!(output.contains("FOO-0001"));
        assert!(!output.contains("[active]"));
        assert!(!output.contains("standalone task"));
        assert!(!output.to_ascii_lowercase().contains("no edges"));
    }

    #[test]
    fn missing_and_unavailable_nodes_explain_the_terminal_reference() {
        let output = super::render(
            &GetTaskDagResponse {
                root_id: "FOO-0001".to_string(),
                nodes: vec![
                    TaskDagNode {
                        value: Some(task_dag_node::Value::Task(TaskDagTaskNode {
                            id: "FOO-0001".to_string(),
                            title: "blocked root".to_string(),
                            status: TaskStatus::Active as i32,
                        })),
                    },
                    TaskDagNode {
                        value: Some(task_dag_node::Value::Missing(
                            pwf_client::pb::TaskDagMissingNode {
                                id: "FOO-0002".to_string(),
                            },
                        )),
                    },
                    TaskDagNode {
                        value: Some(task_dag_node::Value::Unavailable(
                            pwf_client::pb::TaskDagUnavailableNode {
                                id: "AUX-0001".to_string(),
                            },
                        )),
                    },
                ],
                edges: vec![
                    TaskDagEdge {
                        blocker_node_index: 1,
                        dependent_node_index: 0,
                    },
                    TaskDagEdge {
                        blocker_node_index: 2,
                        dependent_node_index: 0,
                    },
                ],
            },
            None,
            false,
        )
        .unwrap();

        assert!(output.contains("FOO-0002 [missing]"));
        assert!(output.contains("AUX-0001 [unavailable]"));
    }

    #[test]
    fn task_identifiers_use_lifecycle_colors_without_coloring_extra_fields() {
        let output = super::render(
            &GetTaskDagResponse {
                root_id: "FOO-0001".to_string(),
                nodes: vec![
                    task_node("FOO-0001", TaskStatus::Active),
                    task_node("FOO-0002", TaskStatus::Done),
                    task_node("FOO-0003", TaskStatus::Cancelled),
                ],
                edges: Vec::new(),
            },
            Some(super::NodeFieldChoice::Status),
            true,
        )
        .unwrap();

        assert!(output.contains("\u{1b}[34mFOO-0001\u{1b}[0m [active]"));
        assert!(output.contains("\u{1b}[32mFOO-0002\u{1b}[0m [done]"));
        assert!(output.contains("\u{1b}[31mFOO-0003\u{1b}[0m [cancelled]"));
    }

    fn task_node(id: &str, status: TaskStatus) -> TaskDagNode {
        TaskDagNode {
            value: Some(task_dag_node::Value::Task(TaskDagTaskNode {
                id: id.to_string(),
                title: "unused title".to_string(),
                status: status as i32,
            })),
        }
    }
}
