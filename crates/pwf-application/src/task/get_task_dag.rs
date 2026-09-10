use std::collections::{BTreeMap, BTreeSet};

use pwf_models::{
    project::{Project, ProjectId},
    task::TaskId,
};
use pwf_wire::{
    project::{GetProject, ProjectStatusFilter},
    task::{GetTaskDag, TaskDag, TaskDagEdge, TaskDagError, TaskDagNode, TaskRecord},
};

use crate::{
    ports::task_vault::TaskVault,
    project::{
        get_project::{self, GetProjectError},
        list_projects,
    },
};

#[derive(Debug, thiserror::Error)]
pub enum GetTaskDagError {
    #[error("Unknown project ID `{project_id}` for task {task_id}")]
    UnknownProjectId {
        task_id: TaskId,
        project_id: ProjectId,
    },
    #[error("task {id} was not found")]
    TaskNotFound { id: TaskId },
    #[error("reading managed projects: {0}")]
    QueryProject(#[source] anyhow::Error),
    #[error("reading root task {id}: {source}")]
    ReadRoot {
        id: TaskId,
        #[source]
        source: anyhow::Error,
    },
    #[error("listing tasks for project {project}: {source}")]
    ListProjectTasks {
        project: ProjectId,
        #[source]
        source: anyhow::Error,
    },
    #[error("task dependency graph contains a cycle: {}", format_path(path))]
    Cycle { path: Vec<TaskId> },
    #[error("task dependency graph exceeds the {max} node limit")]
    NodeLimit { max: usize },
    #[error("task dependency graph exceeds the {max} edge limit")]
    EdgeLimit { max: usize },
    #[error(transparent)]
    InvalidGraph(#[from] TaskDagError),
}

/// Reads one bounded dependency DAG without mutating task or project state.
#[cqrsy::query]
pub async fn execute(
    query: &GetTaskDag,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
) -> Result<TaskDag, GetTaskDagError> {
    let root_project = get_project::execute(
        GetProject::new(query.id.project_id(), ProjectStatusFilter::IncludingPaused),
        pool,
    )
    .await
    .map_err(|error| match error {
        GetProjectError::ProjectNotFound { .. } => GetTaskDagError::UnknownProjectId {
            task_id: query.id.clone(),
            project_id: query.id.project_id().clone(),
        },
        error @ GetProjectError::Unexpected { .. } => {
            GetTaskDagError::QueryProject(anyhow::Error::new(error))
        }
    })?;
    let root = store
        .get_task_record(&root_project, &query.id)
        .map_err(|source| GetTaskDagError::ReadRoot {
            id: query.id.clone(),
            source: anyhow::Error::new(source),
        })?
        .ok_or_else(|| GetTaskDagError::TaskNotFound {
            id: query.id.clone(),
        })?;

    let mut resolver = Resolver::new(store, pool, root_project, root.clone());
    let dependents = if query.mode.includes_blocks() {
        Some(resolver.gather_dependents().await?)
    } else {
        None
    };
    let traversal = Traversal::new(query, resolver, dependents, root);
    traversal.run().await
}

#[derive(Debug, Clone)]
enum ResolvedTask {
    Found(Box<TaskRecord>),
    Missing,
    Unavailable,
}

struct Resolver<'a, Store> {
    store: &'a Store,
    pool: &'a sqlx::SqlitePool,
    projects: BTreeMap<ProjectId, Option<Project>>,
    tasks: BTreeMap<TaskId, ResolvedTask>,
}

impl<'a, Store: TaskVault> Resolver<'a, Store> {
    fn new(
        store: &'a Store,
        pool: &'a sqlx::SqlitePool,
        project: Project,
        root: TaskRecord,
    ) -> Self {
        Self {
            store,
            pool,
            projects: BTreeMap::from([(project.id.clone(), Some(project))]),
            tasks: BTreeMap::from([(root.id.clone(), ResolvedTask::Found(Box::new(root)))]),
        }
    }

    async fn resolve(&mut self, id: &TaskId) -> Result<ResolvedTask, GetTaskDagError> {
        if let Some(task) = self.tasks.get(id) {
            return Ok(task.clone());
        }
        let project_id = id.project_id();
        if !self.projects.contains_key(project_id) {
            let project = match get_project::execute(
                GetProject::new(project_id, ProjectStatusFilter::IncludingPaused),
                self.pool,
            )
            .await
            {
                Ok(project) => Some(project),
                Err(GetProjectError::ProjectNotFound { .. }) => None,
                Err(error) => return Err(GetTaskDagError::QueryProject(anyhow::Error::new(error))),
            };
            self.projects.insert(project_id.clone(), project);
        }
        let resolved = self
            .projects
            .get(project_id)
            .and_then(Option::as_ref)
            .map_or(ResolvedTask::Missing, |project| {
                match self.store.get_task_record(project, id) {
                    Ok(Some(record)) => ResolvedTask::Found(Box::new(record)),
                    Ok(None) => ResolvedTask::Missing,
                    Err(_) => ResolvedTask::Unavailable,
                }
            });
        self.tasks.insert(id.clone(), resolved.clone());
        Ok(resolved)
    }

    async fn gather_dependents(
        &mut self,
    ) -> Result<BTreeMap<TaskId, Vec<TaskId>>, GetTaskDagError> {
        let projects = list_projects::execute(ProjectStatusFilter::IncludingPaused, self.pool)
            .await
            .map_err(|error| GetTaskDagError::QueryProject(anyhow::Error::new(error)))?;
        for project in projects {
            let records = self.store.list_tasks(&project).map_err(|source| {
                GetTaskDagError::ListProjectTasks {
                    project: project.id.clone(),
                    source: anyhow::Error::new(source),
                }
            })?;
            self.projects.insert(project.id.clone(), Some(project));
            for record in records {
                self.cache_scanned_record(record);
            }
        }
        let mut dependents = BTreeMap::<TaskId, Vec<TaskId>>::new();
        for task in self.tasks.values() {
            let ResolvedTask::Found(record) = task else {
                continue;
            };
            let Some(blocked_by) = record.blocked_by.valid() else {
                continue;
            };
            for blocker in blocked_by.iter() {
                dependents
                    .entry(blocker.clone())
                    .or_default()
                    .push(record.id.clone());
            }
        }
        for tasks in dependents.values_mut() {
            tasks.sort();
            tasks.dedup();
        }
        Ok(dependents)
    }

    fn cache_scanned_record(&mut self, record: TaskRecord) {
        let id = record.id.clone();
        let resolved = ResolvedTask::Found(Box::new(record));
        self.tasks.insert(id, resolved);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Complete,
}

struct Traversal<'a, Store> {
    query: &'a GetTaskDag,
    resolver: Resolver<'a, Store>,
    dependents: Option<BTreeMap<TaskId, Vec<TaskId>>>,
    graph: GraphBuilder,
}

impl<'a, Store: TaskVault> Traversal<'a, Store> {
    fn new(
        query: &'a GetTaskDag,
        resolver: Resolver<'a, Store>,
        dependents: Option<BTreeMap<TaskId, Vec<TaskId>>>,
        root: TaskRecord,
    ) -> Self {
        Self {
            query,
            resolver,
            dependents,
            graph: GraphBuilder::new(query.id.clone(), root),
        }
    }

    async fn run(mut self) -> Result<TaskDag, GetTaskDagError> {
        if self.query.mode.includes_blocked_by() {
            self.traverse(Direction::BlockedBy).await?;
        }
        if self.query.mode.includes_blocks() {
            self.traverse(Direction::Blocks).await?;
        }
        self.graph.finish().map_err(Into::into)
    }

    async fn traverse(&mut self, direction: Direction) -> Result<(), GetTaskDagError> {
        let mut states = BTreeMap::new();
        let mut path = Vec::new();
        self.visit(self.query.id.clone(), 0, direction, &mut states, &mut path)
            .await
    }

    async fn visit(
        &mut self,
        id: TaskId,
        depth: u32,
        direction: Direction,
        states: &mut BTreeMap<TaskId, VisitState>,
        path: &mut Vec<TaskId>,
    ) -> Result<(), GetTaskDagError> {
        match states.get(&id) {
            Some(VisitState::Complete) => return Ok(()),
            Some(VisitState::Visiting) => return Err(cycle_error(path, &id)),
            None => {}
        }
        states.insert(id.clone(), VisitState::Visiting);
        path.push(id.clone());

        let current_node_index = self.graph.task_node_index(&id);
        let adjacent = self.adjacent(&id, direction).await?;
        if self.query.depth.is_some_and(|limit| depth >= limit.get()) {
            self.mark_depth_limit(&adjacent, direction, current_node_index)
                .await?;
        } else {
            self.visit_adjacent_nodes(adjacent, depth, direction, current_node_index, states, path)
                .await?;
        }

        path.pop();
        states.insert(id, VisitState::Complete);
        Ok(())
    }

    async fn mark_depth_limit(
        &mut self,
        adjacent: &[TaskId],
        direction: Direction,
        current_node_index: usize,
    ) -> Result<(), GetTaskDagError> {
        if !self.has_visible(adjacent).await? {
            return Ok(());
        }
        let marker_node_index = self.graph.add_depth_limit()?;
        self.graph
            .add_directional_edge(direction, current_node_index, marker_node_index)
    }

    async fn visit_adjacent_nodes(
        &mut self,
        adjacent: Vec<TaskId>,
        depth: u32,
        direction: Direction,
        current_node_index: usize,
        states: &mut BTreeMap<TaskId, VisitState>,
        path: &mut Vec<TaskId>,
    ) -> Result<(), GetTaskDagError> {
        for adjacent_id in adjacent {
            self.visit_adjacent_node(
                adjacent_id,
                depth,
                direction,
                current_node_index,
                states,
                path,
            )
            .await?;
        }
        Ok(())
    }

    async fn visit_adjacent_node(
        &mut self,
        adjacent_id: TaskId,
        depth: u32,
        direction: Direction,
        current_node_index: usize,
        states: &mut BTreeMap<TaskId, VisitState>,
        path: &mut Vec<TaskId>,
    ) -> Result<(), GetTaskDagError> {
        let resolved = self.resolver.resolve(&adjacent_id).await?;
        let traversable = matches!(resolved, ResolvedTask::Found(_));
        let Some(adjacent_node_index) = self.add_visible_node(&adjacent_id, resolved)? else {
            return Ok(());
        };
        self.graph
            .add_directional_edge(direction, current_node_index, adjacent_node_index)?;
        if traversable {
            Box::pin(self.visit(adjacent_id, depth + 1, direction, states, path)).await?;
        }
        Ok(())
    }

    async fn adjacent(
        &mut self,
        id: &TaskId,
        direction: Direction,
    ) -> Result<Vec<TaskId>, GetTaskDagError> {
        Ok(match direction {
            Direction::BlockedBy => match self.resolver.resolve(id).await? {
                ResolvedTask::Found(record) => record
                    .blocked_by
                    .valid()
                    .map(|blocked_by| blocked_by.iter().cloned().collect())
                    .unwrap_or_default(),
                ResolvedTask::Missing | ResolvedTask::Unavailable => Vec::new(),
            },
            Direction::Blocks => self
                .dependents
                .as_ref()
                .and_then(|dependents| dependents.get(id))
                .cloned()
                .unwrap_or_default(),
        })
    }

    async fn has_visible(&mut self, adjacent: &[TaskId]) -> Result<bool, GetTaskDagError> {
        for id in adjacent {
            let visible = match self.resolver.resolve(id).await? {
                ResolvedTask::Found(record) => self.query.status.includes(record.status),
                ResolvedTask::Missing | ResolvedTask::Unavailable => true,
            };
            if visible {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn add_visible_node(
        &mut self,
        id: &TaskId,
        resolved: ResolvedTask,
    ) -> Result<Option<usize>, GetTaskDagError> {
        match resolved {
            ResolvedTask::Found(record) if self.query.status.includes(record.status) => {
                self.graph.add_task(*record).map(Some)
            }
            ResolvedTask::Found(_) => Ok(None),
            ResolvedTask::Missing => self.graph.add_missing(id.clone()).map(Some),
            ResolvedTask::Unavailable => self.graph.add_unavailable(id.clone()).map(Some),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    BlockedBy,
    Blocks,
}

struct GraphBuilder {
    root_id: TaskId,
    nodes: Vec<TaskDagNode>,
    task_node_indexes: BTreeMap<TaskId, usize>,
    edges: Vec<TaskDagEdge>,
    unique_edges: BTreeSet<TaskDagEdge>,
}

impl GraphBuilder {
    fn new(root_id: TaskId, root: TaskRecord) -> Self {
        Self {
            root_id: root_id.clone(),
            nodes: vec![task_node(root)],
            task_node_indexes: BTreeMap::from([(root_id, 0)]),
            edges: Vec::new(),
            unique_edges: BTreeSet::new(),
        }
    }

    fn finish(self) -> Result<TaskDag, TaskDagError> {
        TaskDag::try_new(self.root_id, self.nodes, self.edges)
    }

    fn task_node_index(&self, id: &TaskId) -> usize {
        self.task_node_indexes[id]
    }

    fn add_task(&mut self, task: TaskRecord) -> Result<usize, GetTaskDagError> {
        self.add_task_node(task.id.clone(), task_node(task))
    }

    fn add_missing(&mut self, id: TaskId) -> Result<usize, GetTaskDagError> {
        self.add_task_node(id.clone(), TaskDagNode::Missing { id })
    }

    fn add_unavailable(&mut self, id: TaskId) -> Result<usize, GetTaskDagError> {
        self.add_task_node(id.clone(), TaskDagNode::Unavailable { id })
    }

    fn add_task_node(&mut self, id: TaskId, node: TaskDagNode) -> Result<usize, GetTaskDagError> {
        if let Some(index) = self.task_node_indexes.get(&id) {
            return Ok(*index);
        }
        let index = self.add_node(node)?;
        self.task_node_indexes.insert(id, index);
        Ok(index)
    }

    fn add_depth_limit(&mut self) -> Result<usize, GetTaskDagError> {
        self.add_node(TaskDagNode::DepthLimit)
    }

    fn add_node(&mut self, node: TaskDagNode) -> Result<usize, GetTaskDagError> {
        if self.nodes.len() >= TaskDag::NODE_COUNT_MAX {
            return Err(GetTaskDagError::NodeLimit {
                max: TaskDag::NODE_COUNT_MAX,
            });
        }
        let index = self.nodes.len();
        self.nodes.push(node);
        Ok(index)
    }

    fn add_directional_edge(
        &mut self,
        direction: Direction,
        current_node_index: usize,
        adjacent_node_index: usize,
    ) -> Result<(), GetTaskDagError> {
        let edge = match direction {
            Direction::BlockedBy => TaskDagEdge {
                blocker_node_index: adjacent_node_index,
                dependent_node_index: current_node_index,
            },
            Direction::Blocks => TaskDagEdge {
                blocker_node_index: current_node_index,
                dependent_node_index: adjacent_node_index,
            },
        };
        if self.unique_edges.contains(&edge) {
            return Ok(());
        }
        if self.edges.len() >= TaskDag::EDGE_COUNT_MAX {
            return Err(GetTaskDagError::EdgeLimit {
                max: TaskDag::EDGE_COUNT_MAX,
            });
        }
        self.unique_edges.insert(edge);
        self.edges.push(edge);
        Ok(())
    }
}

fn task_node(record: TaskRecord) -> TaskDagNode {
    TaskDagNode::Task {
        id: record.id,
        title: record.title,
        status: record.status,
    }
}

fn cycle_error(path: &[TaskId], repeated: &TaskId) -> GetTaskDagError {
    let start = path.iter().position(|id| id == repeated).unwrap_or(0);
    let mut cycle = path[start..].to_vec();
    cycle.push(repeated.clone());
    GetTaskDagError::Cycle { path: cycle }
}

fn format_path(path: &[TaskId]) -> String {
    path.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" -> ")
}
