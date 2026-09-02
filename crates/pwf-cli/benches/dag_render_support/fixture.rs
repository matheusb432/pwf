use pwf_client::task::{TaskDag, TaskDagEdge, TaskDagNode};
use pwf_models::task::{TaskId, TaskStatus};

pub const FIXTURE_SCHEMA_VERSION: u32 = 1;

pub struct DagRenderFixture {
    pub name: &'static str,
    pub task_dag: TaskDag,
}

impl DagRenderFixture {
    fn new(name: &'static str, task_dag: TaskDag, node_count: usize, edge_count: usize) -> Self {
        require_condition(
            task_dag.nodes().len() == node_count,
            "fixture node count matches its manifest",
        );
        require_condition(
            task_dag.edges().len() == edge_count,
            "fixture edge count matches its manifest",
        );
        Self { name, task_dag }
    }
}

#[derive(Clone, Copy)]
pub enum DagRenderColor {
    Plain,
    Ansi,
}

impl DagRenderColor {
    pub const ALL: [Self; 2] = [Self::Plain, Self::Ansi];

    pub const fn color_on(self) -> bool {
        match self {
            Self::Plain => false,
            Self::Ansi => true,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Ansi => "color",
        }
    }
}

pub fn fixtures() -> [DagRenderFixture; 3] {
    [
        DagRenderFixture::new("chain-8", chain_task_dag(8), 8, 7),
        DagRenderFixture::new("layered-128-360", layered_task_dag(16, 8, 3), 128, 360),
        DagRenderFixture::new("limit-512-2048", limit_task_dag(), 512, 2_048),
    ]
}

fn chain_task_dag(node_count: usize) -> TaskDag {
    let edges = (1..node_count)
        .map(|dependent| edge(dependent - 1, dependent))
        .collect();
    task_dag(node_count, node_count - 1, edges)
}

fn layered_task_dag(
    layer_count: usize,
    nodes_per_layer: usize,
    blockers_per_node: usize,
) -> TaskDag {
    let edges = layered_edges(layer_count, nodes_per_layer, blockers_per_node);
    task_dag(
        layer_count * nodes_per_layer,
        layer_count * nodes_per_layer - 1,
        edges,
    )
}

fn layered_edges(
    layer_count: usize,
    nodes_per_layer: usize,
    blockers_per_node: usize,
) -> Vec<TaskDagEdge> {
    let mut edges = Vec::new();
    for layer in 1..layer_count {
        for dependent_offset in 0..nodes_per_layer {
            let dependent = layer * nodes_per_layer + dependent_offset;
            for blocker_offset in 0..blockers_per_node {
                let blocker = (layer - 1) * nodes_per_layer
                    + (dependent_offset + blocker_offset) % nodes_per_layer;
                edges.push(edge(blocker, dependent));
            }
        }
    }
    edges
}

fn limit_task_dag() -> TaskDag {
    let mut edges = layered_edges(32, 16, 4);
    for dependent_offset in 0..16 {
        for blocker_offset in 0..4 {
            let blocker = blocker_offset;
            let dependent = 32 + dependent_offset;
            edges.push(edge(blocker, dependent));
        }
    }
    task_dag(512, 511, edges)
}

fn task_dag(node_count: usize, root_node_index: usize, edges: Vec<TaskDagEdge>) -> TaskDag {
    require(
        TaskDag::try_new(
            task_id(root_node_index),
            (0..node_count).map(task_node).collect(),
            edges,
        ),
        "constructing a fixture task DAG",
    )
}

fn task_node(node_index: usize) -> TaskDagNode {
    TaskDagNode::Task {
        id: task_id(node_index),
        title: format!("benchmark task {node_index:04}"),
        status: TaskStatus::Active,
    }
}

fn edge(blocker_node_index: usize, dependent_node_index: usize) -> TaskDagEdge {
    TaskDagEdge {
        blocker_node_index,
        dependent_node_index,
    }
}

fn task_id(node_index: usize) -> TaskId {
    require(
        TaskId::try_new(format!("PWF-{node_index:04}")),
        "constructing a fixture task ID",
    )
}

fn task_ids(task_dag: &TaskDag) -> impl Iterator<Item = &TaskId> {
    task_dag.nodes().iter().filter_map(TaskDagNode::task_id)
}

pub fn validate_renderer(task_dag: &TaskDag, fixture_name: &str) {
    for color in DagRenderColor::ALL {
        let rendered = pwf_cli::task::benchmark_dag_render(task_dag.clone(), color.color_on());
        for id in task_ids(task_dag) {
            require_condition(
                rendered.contains(id.as_ref()),
                &format!(
                    "renderer output contains {id} in {fixture_name}/{}",
                    color.name()
                ),
            );
        }
        require_condition(
            rendered.contains("\u{1b}[") == color.color_on(),
            &format!(
                "renderer output matches the requested color mode in {fixture_name}/{}",
                color.name()
            ),
        );
    }
}

pub fn require<T, Error>(result: Result<T, Error>, context: &str) -> T
where
    Error: std::fmt::Display,
{
    match result {
        Ok(value) => value,
        Err(error) => {
            eprintln!("benchmark setup failed while {context}: {error}");
            std::process::exit(1);
        }
    }
}

fn require_condition(condition: bool, context: &str) {
    if !condition {
        eprintln!("benchmark setup failed: {context}");
        std::process::exit(1);
    }
}
