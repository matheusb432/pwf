use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::Debug,
};

use pwf_application::{
    ports::task_vault::TaskDependencyRecord,
    task::{
        get_task_dag,
        read_task_dependencies::{self, ReadTaskDependencies},
    },
};
use pwf_infra::{database, obsidian::ObsidianStore};
use pwf_models::{
    project::HomeDirectory,
    task::{BlockedBy, TaskId, TaskStatus},
};
use pwf_wire::task::{GetTaskDag, StatusFilter, TaskDag, TaskDagMode, TaskDagNode};

#[derive(Clone, Copy)]
pub enum Shape {
    Chain,
    Shared,
    CrossProject,
    Sparse,
    LargeBody,
}

impl Shape {
    pub const ALL: [Self; 5] = [
        Self::Chain,
        Self::Shared,
        Self::CrossProject,
        Self::Sparse,
        Self::LargeBody,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Chain => "chain-64",
            Self::Shared => "shared-64",
            Self::CrossProject => "cross-4x16",
            Self::Sparse => "sparse-3-of-1024",
            Self::LargeBody => "chain-64-body-64kib",
        }
    }

    fn project_count(self) -> usize {
        if matches!(self, Self::CrossProject) {
            4
        } else {
            1
        }
    }

    fn task_count(self) -> usize {
        if matches!(self, Self::Sparse) {
            1024
        } else {
            64
        }
    }

    fn reached_count(self) -> usize {
        if matches!(self, Self::Sparse) { 3 } else { 64 }
    }

    fn id(self, number: usize) -> TaskId {
        let project = ["AAA", "BBB", "CCC", "DDD"][number % self.project_count()];
        require(
            format!("{project}-{number:04}").parse(),
            "parsing fixture task ID",
        )
    }

    fn blockers(self, number: usize) -> Vec<TaskId> {
        if number == 0 || number >= self.reached_count() {
            return Vec::new();
        }
        if matches!(self, Self::Shared) && number == 63 {
            return (1..63).map(|number| self.id(number)).collect();
        }
        let blocker = if matches!(self, Self::Shared) {
            0
        } else {
            number - 1
        };
        vec![self.id(blocker)]
    }
}

pub struct Workload {
    runtime: tokio::runtime::Runtime,
    pool: sqlx::SqlitePool,
    projects: pwf_infra::project_store::SqliteProjectStore,
    store: ObsidianStore,
    shape: Shape,
    root: TaskId,
    target: TaskId,
    blockers: BlockedBy,
    _directory: tempfile::TempDir,
}

impl Workload {
    pub fn new(shape: Shape, watched: bool) -> Self {
        let runtime = require(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build(),
            "building runtime",
        );
        let directory = require(tempfile::tempdir(), "creating fixture directory");
        let pool = runtime.block_on(async {
            let pool = require(database::build_migration_pool(&directory.path().join("registry.sqlite3")).await, "opening registry");
            require(database::migrate_database(&pool).await, "migrating registry");
            for project in ["AAA", "BBB", "CCC", "DDD"].into_iter().take(shape.project_count()) {
                let path = directory.path().join(project);
                require(std::fs::create_dir(&path), "creating task directory");
                require(sqlx::query("INSERT INTO projects (id, title, tasks_kind, tasks_path, created_at) VALUES (?, ?, 'directory', ?, '2026-09-13T00:00:00Z')")
                    .bind(project).bind(project.to_lowercase()).bind(path.to_string_lossy().as_ref()).execute(&pool).await, "registering project");
            }
            pool
        });
        for number in 0..shape.task_count() {
            let id = shape.id(number);
            let blockers = shape
                .blockers(number)
                .iter()
                .map(|id| format!("\"[[{id}]]\""))
                .collect::<Vec<_>>()
                .join(", ");
            let body = "x".repeat(if matches!(shape, Shape::LargeBody) {
                65_536
            } else {
                256
            });
            let source = format!(
                "---\nid: {id}\ntitle: task {number}\nstatus: active\ncreated_at: 2026-09-13T00:00:00Z\nblocked_by: [{blockers}]\n---\n{body}\n"
            );
            let path = directory
                .path()
                .join(id.project_id().as_ref())
                .join(format!("note-{number}.md"));
            require(std::fs::write(path, source), "writing task fixture");
        }
        let home = HomeDirectory::new(directory.path().to_path_buf());
        let store = if watched {
            ObsidianStore::with_watched_tasks(home)
        } else {
            ObsidianStore::new(home)
        };
        let root = shape.id(shape.reached_count() - 1);
        let blockers = require(BlockedBy::try_new(vec![root.clone()]), "building blockers");
        Self {
            runtime,
            projects: pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
            pool,
            store,
            shape,
            root,
            target: shape.id(9999),
            blockers,
            _directory: directory,
        }
    }

    pub fn dependencies(&self) -> HashMap<TaskId, TaskDependencyRecord> {
        require(
            self.runtime.block_on(read_task_dependencies::execute(
                ReadTaskDependencies {
                    target: &self.target,
                    blockers: &self.blockers,
                },
                &self.store,
                &self.projects,
            )),
            "gathering dependencies",
        )
    }

    pub fn graph(&self, mode: TaskDagMode) -> TaskDag {
        let root = if mode == TaskDagMode::Blocks {
            self.shape.id(0)
        } else {
            self.root.clone()
        };
        require(
            self.runtime.block_on(get_task_dag::execute(
                &GetTaskDag {
                    id: root,
                    depth: None,
                    status: StatusFilter::All,
                    mode,
                },
                &self.store,
                &self.projects,
            )),
            "building graph",
        )
    }

    pub fn validate(&self) {
        let expected = self.shape.reached_count();
        let dependencies = self.dependencies();
        assert_eq!(dependencies.len(), expected);
        for number in 0..expected {
            let record = require(
                dependencies
                    .get(&self.shape.id(number))
                    .ok_or("missing task"),
                "checking gathered membership",
            );
            let blockers = record
                .blocked_by
                .valid()
                .map(|values| values.iter().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            assert_eq!(blockers, self.shape.blockers(number));
        }
        for mode in [
            TaskDagMode::BlockedBy,
            TaskDagMode::Blocks,
            TaskDagMode::Full,
        ] {
            self.validate_graph(&self.graph(mode));
        }
    }

    fn validate_graph(&self, graph: &TaskDag) {
        let expected = self.shape.reached_count();
        let expected_nodes = (0..expected)
            .map(|number| {
                (
                    self.shape.id(number),
                    (format!("task {number}"), TaskStatus::Active),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let nodes = graph
            .nodes()
            .iter()
            .filter_map(|node| match node {
                TaskDagNode::Task { id, title, status } => {
                    Some((id.clone(), (title.clone(), *status)))
                }
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(nodes, expected_nodes);
        assert_eq!(graph.nodes().len(), expected);
        let expected_edges = (0..expected)
            .flat_map(|number| {
                self.shape
                    .blockers(number)
                    .into_iter()
                    .map(move |blocker| (blocker, self.shape.id(number)))
            })
            .collect::<BTreeSet<_>>();
        let edges = graph
            .edges()
            .iter()
            .map(|edge| {
                let blocker = require(
                    graph.nodes()[edge.blocker_node_index]
                        .task_id()
                        .cloned()
                        .ok_or("diagnostic node"),
                    "checking blocker identity",
                );
                let dependent = require(
                    graph.nodes()[edge.dependent_node_index]
                        .task_id()
                        .cloned()
                        .ok_or("diagnostic node"),
                    "checking dependent identity",
                );
                (blocker, dependent)
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(edges, expected_edges);
    }
}

impl Drop for Workload {
    fn drop(&mut self) {
        self.runtime.block_on(self.pool.close());
    }
}

fn require<T>(result: Result<T, impl Debug>, context: &str) -> T {
    result.unwrap_or_else(|error| {
        eprintln!("{context}: {error:?}");
        std::process::exit(1);
    })
}
