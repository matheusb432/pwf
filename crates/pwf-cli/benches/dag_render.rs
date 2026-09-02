use std::{hint::black_box, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use mermaid_text::{Direction, Edge, Graph, Node, NodeShape, layout::layered::LayoutConfig};
use pwf_client::task::{TaskDag, TaskDagEdge, TaskDagNode};
use pwf_models::task::{TaskId, TaskStatus};

const FIXTURE_SCHEMA_VERSION: u32 = 1;
const SAMPLE_SIZE: usize = 10;

struct DagRenderFixture {
    edge_count: usize,
    graph: Graph,
    name: &'static str,
    node_count: usize,
}

impl DagRenderFixture {
    fn new(name: &'static str, task_dag: &TaskDag, node_count: usize, edge_count: usize) -> Self {
        require_condition(
            task_dag.nodes.len() == node_count,
            "fixture node count matches its manifest",
        );
        require_condition(
            task_dag.edges.len() == edge_count,
            "fixture edge count matches its manifest",
        );
        validate_task_dag(task_dag);

        let graph = graph_from_task_dag(task_dag);
        validate_renderer(task_dag, &graph, name);
        Self {
            edge_count,
            graph,
            name,
            node_count,
        }
    }
}

fn dag_render(criterion: &mut Criterion) {
    eprintln!("dag-render fixture_schema={FIXTURE_SCHEMA_VERSION}");

    let mut group = criterion.benchmark_group("dag-render");
    for fixture in fixtures() {
        group.throughput(Throughput::Elements(require(
            u64::try_from(fixture.node_count + fixture.edge_count),
            "converting the fixture element count to u64",
        )));
        group.bench_with_input(
            BenchmarkId::from_parameter(fixture.name),
            &fixture,
            |bencher, fixture| {
                bencher.iter(|| black_box(render_low_level(black_box(&fixture.graph))));
            },
        );
    }
    group.finish();
}

fn fixtures() -> [DagRenderFixture; 3] {
    [
        DagRenderFixture::new("chain-8", &chain_task_dag(8), 8, 7),
        DagRenderFixture::new("layered-128-360", &layered_task_dag(16, 8, 3), 128, 360),
        DagRenderFixture::new("limit-512-2048", &limit_task_dag(), 512, 2_048),
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

    task_dag(
        layer_count * nodes_per_layer,
        layer_count * nodes_per_layer - 1,
        edges,
    )
}

fn limit_task_dag() -> TaskDag {
    let mut task_dag = layered_task_dag(32, 16, 4);
    for dependent_offset in 0..16 {
        for blocker_offset in 0..4 {
            let blocker = blocker_offset;
            let dependent = 32 + dependent_offset;
            task_dag.edges.push(edge(blocker, dependent));
        }
    }
    task_dag
}

fn task_dag(node_count: usize, root_node_index: usize, edges: Vec<TaskDagEdge>) -> TaskDag {
    TaskDag {
        root_id: task_id(root_node_index),
        nodes: (0..node_count).map(task_node).collect(),
        edges,
    }
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

fn validate_task_dag(task_dag: &TaskDag) {
    require_condition(
        task_ids(task_dag).any(|id| id == &task_dag.root_id),
        "fixture contains its root task",
    );
    for edge in &task_dag.edges {
        require_condition(
            edge.blocker_node_index < task_dag.nodes.len(),
            "fixture blocker index references a task",
        );
        require_condition(
            edge.dependent_node_index < task_dag.nodes.len(),
            "fixture dependent index references a task",
        );
    }
}

fn task_ids(task_dag: &TaskDag) -> impl Iterator<Item = &TaskId> {
    task_dag.nodes.iter().map(|node| {
        let TaskDagNode::Task { id, .. } = node else {
            eprintln!("benchmark setup failed: fixtures contain only task nodes");
            std::process::exit(1);
        };
        id
    })
}

fn validate_renderer(task_dag: &TaskDag, graph: &Graph, fixture_name: &str) {
    let rendered = render_low_level(graph);
    for id in task_ids(task_dag) {
        require_condition(
            rendered.contains(id.as_ref()),
            &format!("low-level renderer output contains {id} in {fixture_name}"),
        );
    }
}

fn graph_from_task_dag(task_dag: &TaskDag) -> Graph {
    let mut graph = Graph::new(Direction::LeftToRight);
    for (node_index, id) in task_ids(task_dag).enumerate() {
        let shape = if id == &task_dag.root_id {
            NodeShape::Rounded
        } else {
            NodeShape::Rectangle
        };
        graph
            .nodes
            .push(Node::new(node_key(node_index), id.to_string(), shape));
    }
    for edge in &task_dag.edges {
        graph.edges.push(Edge::new(
            node_key(edge.blocker_node_index),
            node_key(edge.dependent_node_index),
            None,
        ));
    }
    graph
}

fn render_low_level(graph: &Graph) -> String {
    let positions =
        mermaid_text::layout::layered::layout(graph, &LayoutConfig::default()).positions;
    let subgraph_bounds =
        mermaid_text::layout::subgraph::compute_subgraph_bounds(graph, &positions);
    mermaid_text::render::render(graph, &positions, &subgraph_bounds)
}

fn node_key(node_index: usize) -> String {
    format!("n{node_index}")
}

fn require<T, Error>(result: Result<T, Error>, context: &str) -> T
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

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(SAMPLE_SIZE)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = dag_render
}
criterion_main!(benches);
