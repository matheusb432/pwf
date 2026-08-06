use std::path::PathBuf;

use pwf_models::{
    project::{Project, ProjectSelector},
    task::{EffortTier, ProjectName, Tags, TaskId, TaskStatus},
};
#[cfg(test)]
use pwf_wire::task::PrerequisiteStatus;
use pwf_wire::{project::ProjectStatusFilter, task::TaskView};

use super::{prerequisites, tags, task_view};
use crate::{
    ports::{
        project_task_location::ProjectTaskLocationClient,
        task_record::{TaskRecord, TaskStore},
    },
    project::{
        get_active_project::{self, GetActiveProject},
        get_project::GetProjectError,
        list_projects::{self, ListProjects},
        resolve_project::{self, ResolveProject},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListTasksOk {
    pub tasks: Vec<TaskView>,
    pub hidden: usize,
    pub project: Option<ProjectName>,
    pub project_task_path: Option<PathBuf>,
    pub status_filter: StatusFilter,
    pub grouped: bool,
}

/// Selects one lifecycle status or includes every lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusFilter {
    Exact(TaskStatus),
    All,
}

impl StatusFilter {
    #[must_use]
    pub fn includes(self, status: TaskStatus) -> bool {
        match self {
            Self::Exact(expected) => expected == status,
            Self::All => true,
        }
    }
}

impl Default for StatusFilter {
    fn default() -> Self {
        Self::Exact(TaskStatus::Active)
    }
}

/// Selects an explicit task index section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListSection {
    /// Includes only tasks in the normalized `Human` section.
    Human,
    /// Includes only tasks in the normalized `Future` section.
    Future,
}

/// Selects the defaults used by the direct list command or project compatibility route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListMode {
    #[default]
    Direct,
    ProjectRoute,
}

/// Selects the primary list ordering field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderField {
    /// Orders by the persisted creation value.
    Created,
    /// Orders by the numeric task-id suffix.
    Id,
    /// Orders by project name, then by newest task id within each project.
    ProjectId,
}

/// Selects ascending or descending list ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderDirection {
    /// Orders the selected field from lower to higher values.
    Asc,
    /// Orders the selected field from higher to lower values.
    Desc,
}

/// Combines the list ordering field and direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderSpec {
    /// Primary field used to order listed tasks.
    pub field: OrderField,
    /// Direction applied to the primary ordering field.
    pub direction: OrderDirection,
}

impl Default for OrderSpec {
    fn default() -> Self {
        Self {
            field: OrderField::Created,
            direction: OrderDirection::Desc,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ListTasks {
    pub project_selector: Option<ProjectSelector>,
    pub section: Option<ListSection>,
    pub all: bool,
    /// Explicit task cap. Omission uses the mode-specific default.
    pub number: Option<usize>,
    pub effort: Option<EffortTier>,
    pub tags: Option<Tags>,
    pub order: Option<OrderSpec>,
    /// Explicit lifecycle filter. Omission uses the mode-specific default.
    pub status: Option<StatusFilter>,
    /// Projects prerequisite statuses for long-list rendering when enabled.
    pub include_prerequisite_statuses: bool,
    pub mode: ListMode,
}

/// Retains invalid requested or persisted tag text for list diagnostics.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct TagParseError {
    raw: String,
    message: String,
}

impl TagParseError {
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }
}

impl From<tags::ParseTagsError> for TagParseError {
    fn from(error: tags::ParseTagsError) -> Self {
        Self {
            raw: error.raw().to_string(),
            message: error.to_string(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ListTasksError {
    #[error("task read failed: {0}")]
    ReadStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("managed project task path read failed: {0}")]
    ReadProjectTaskPath(Box<dyn std::error::Error + Send + Sync>),
    #[error("task {id} has invalid tags frontmatter: {source}")]
    InvalidTags {
        id: TaskId,
        #[source]
        source: TagParseError,
    },
    #[error(transparent)]
    ResolveProject(#[from] crate::project::resolve_project::ResolveProjectError),
    #[error("{0}")]
    QueryProject(#[source] Box<dyn std::error::Error + Send + Sync>),
}

struct ResolvedListTasks {
    project: Option<Project>,
    scope: ListScope,
    cap: Option<usize>,
    effort: Option<EffortTier>,
    tags: Option<Tags>,
    order: OrderSpec,
    status_filter: StatusFilter,
    include_prerequisite_statuses: bool,
}

#[derive(Clone, Copy)]
enum ListScope {
    Default,
    HumanOnly,
    FutureOnly,
    All,
}

#[cqrsy::query]
pub async fn execute(
    query: &ListTasks,
    store: &impl TaskStore,
    pool: &sqlx::SqlitePool,
    task_locations: &impl ProjectTaskLocationClient,
) -> Result<ListTasksOk, ListTasksError> {
    let selected = match query.project_selector.as_ref() {
        Some(selector) => Some(
            resolve_project::execute(
                ResolveProject {
                    selector: selector.clone(),
                    status: ProjectStatusFilter::ACTIVE,
                },
                pool,
            )
            .await?,
        ),
        None => None,
    };
    let selected_records = selected
        .as_ref()
        .map(|project| {
            store
                .list(project)
                .map_err(|error| ListTasksError::ReadStore(Box::new(error)))
        })
        .transpose()?;
    let projects = match selected.as_ref() {
        Some(project) => {
            let mut projects = vec![project.clone()];
            if query.include_prerequisite_statuses {
                for id in prerequisites::referenced_project_ids(
                    selected_records
                        .iter()
                        .flatten()
                        .filter_map(|record| record.prereq.as_deref()),
                ) {
                    if projects.iter().any(|project| project.id == id) {
                        continue;
                    }
                    match get_active_project::execute(GetActiveProject { id }, pool).await {
                        Ok(project) => projects.push(project),
                        Err(GetProjectError::ProjectNotFound { .. }) => {}
                        Err(error) => {
                            return Err(ListTasksError::QueryProject(Box::new(error)));
                        }
                    }
                }
            }
            projects
        }
        None => list_projects::execute(
            ListProjects {
                status: ProjectStatusFilter::ACTIVE,
            },
            pool,
        )
        .await
        .map_err(|error| ListTasksError::QueryProject(Box::new(error)))?,
    };
    let query = resolve_query(query, selected);
    let project_task_path = query
        .project
        .as_ref()
        .map(|project| task_locations.project_task_path(project))
        .transpose()
        .map_err(|source| ListTasksError::ReadProjectTaskPath(Box::new(source)))?;
    let mut tasks = collect_list_tasks(&query, store, &projects, selected_records.as_deref())?;

    tasks.retain(|task| scope_includes(query.scope, task.section.as_deref()));
    tasks.retain(|task| effort_matches(task, query.effort));

    tasks = retain_matching_tags(tasks, query.tags.as_ref())?;

    if scope_groups_output(query.scope) {
        sort_by_group_then_order(&mut tasks, query.order);
    } else {
        sort_by_order(&mut tasks, query.order);
    }

    let (mut tasks, hidden) = apply_cap(tasks, query.cap);

    if query.include_prerequisite_statuses {
        for task in &mut tasks {
            task.prerequisite_statuses =
                task.prerequisites.as_ref().map_or_else(Vec::new, |value| {
                    prerequisites::statuses(value, store, &projects)
                });
        }
    }

    Ok(ListTasksOk {
        tasks,
        hidden,
        project: query.project.map(|project| project.title),
        project_task_path,
        status_filter: query.status_filter,
        grouped: scope_groups_output(query.scope),
    })
}

fn retain_matching_tags(
    tasks: Vec<TaskView>,
    requested: Option<&Tags>,
) -> Result<Vec<TaskView>, ListTasksError> {
    let Some(requested) = requested else {
        return Ok(tasks);
    };
    let mut matched = Vec::with_capacity(tasks.len());
    for task in tasks {
        let Some(raw) = task.tags.as_deref() else {
            continue;
        };
        let stored =
            tags::parse_frontmatter(raw).map_err(|source| ListTasksError::InvalidTags {
                id: task.id.clone(),
                source: source.into(),
            })?;
        if tags::contains_all(&stored, requested) {
            matched.push(task);
        }
    }
    Ok(matched)
}

fn resolve_query(query: &ListTasks, project: Option<Project>) -> ResolvedListTasks {
    let scope = if query.all {
        ListScope::All
    } else {
        match query.section {
            None => ListScope::Default,
            Some(ListSection::Human) => ListScope::HumanOnly,
            Some(ListSection::Future) => ListScope::FutureOnly,
        }
    };
    let cap = query.number.or((!query.all).then_some(10));
    let order = query.order.unwrap_or(match query.mode {
        ListMode::Direct => OrderSpec::default(),
        ListMode::ProjectRoute => OrderSpec {
            field: OrderField::ProjectId,
            direction: OrderDirection::Asc,
        },
    });
    let status_filter = query.status.unwrap_or(if query.all {
        StatusFilter::All
    } else {
        StatusFilter::default()
    });

    ResolvedListTasks {
        project,
        scope,
        cap,
        effort: query.effort,
        tags: query.tags.clone(),
        order,
        status_filter,
        include_prerequisite_statuses: query.include_prerequisite_statuses,
    }
}

/// Reads and enriches listable lifecycle records from one project or every project in name order.
fn collect_list_tasks(
    query: &ResolvedListTasks,
    store: &impl TaskStore,
    projects: &[Project],
    selected_records: Option<&[TaskRecord]>,
) -> Result<Vec<TaskView>, ListTasksError> {
    let scan: Vec<&Project> = match query.project.as_ref() {
        Some(project) => vec![project],
        None => projects.iter().collect(),
    };

    let mut tasks = Vec::new();
    for project in scan {
        let records = if query.project.is_some() {
            selected_records.map_or_else(Vec::new, <[TaskRecord]>::to_vec)
        } else {
            store
                .list(project)
                .map_err(|error| ListTasksError::ReadStore(Box::new(error)))?
        };
        for record in records {
            if !query.status_filter.includes(record.status) {
                continue;
            }
            tasks.push(
                task_view::enrich(&record, project.source.value())
                    .into_task_view(project.title.to_string()),
            );
        }
    }
    Ok(tasks)
}

fn scope_includes(scope: ListScope, section: Option<&str>) -> bool {
    match scope {
        ListScope::Default => section.is_none(),
        ListScope::HumanOnly => matches!(section, Some("Human")),
        ListScope::FutureOnly => matches!(section, Some("Future")),
        ListScope::All => true,
    }
}

fn scope_groups_output(scope: ListScope) -> bool {
    matches!(scope, ListScope::All)
}

fn section_group_rank(section: Option<&str>) -> u8 {
    match section {
        None => 0,
        Some("Low-prio") => 1,
        Some("Human") => 2,
        Some("Future") => 3,
        Some(_) => 4,
    }
}

fn effort_matches(task: &TaskView, wanted: Option<EffortTier>) -> bool {
    let Some(wanted) = wanted else { return true };
    task.effort.as_deref().and_then(parse_effort_tier) == Some(wanted)
}

fn parse_effort_tier(raw: &str) -> Option<EffortTier> {
    raw.trim().parse().ok()
}

fn id_suffix(id: &TaskId) -> u64 {
    id.as_ref()
        .rsplit_once('-')
        .and_then(|(_, digits)| digits.parse().ok())
        .unwrap_or(0)
}

fn created_key(task: &TaskView) -> &str {
    task.created.as_deref().unwrap_or("")
}

fn task_order_cmp(order: OrderSpec, a: &TaskView, b: &TaskView) -> std::cmp::Ordering {
    match order.field {
        OrderField::Created => {
            let ascending = created_key(a)
                .cmp(created_key(b))
                .then_with(|| a.id.cmp(&b.id));
            match order.direction {
                OrderDirection::Asc => ascending,
                OrderDirection::Desc => ascending.reverse(),
            }
        }
        OrderField::Id => {
            let ascending = id_suffix(&a.id)
                .cmp(&id_suffix(&b.id))
                .then_with(|| a.id.cmp(&b.id));
            match order.direction {
                OrderDirection::Asc => ascending,
                OrderDirection::Desc => ascending.reverse(),
            }
        }
        OrderField::ProjectId => {
            let project_cmp = match order.direction {
                OrderDirection::Asc => a.project.cmp(&b.project),
                OrderDirection::Desc => b.project.cmp(&a.project),
            };
            project_cmp
                .then_with(|| id_suffix(&b.id).cmp(&id_suffix(&a.id)))
                .then_with(|| b.id.cmp(&a.id))
        }
    }
}

fn sort_by_order(tasks: &mut [TaskView], order: OrderSpec) {
    tasks.sort_by(|a, b| task_order_cmp(order, a, b));
}

fn sort_by_group_then_order(tasks: &mut [TaskView], order: OrderSpec) {
    tasks.sort_by(|a, b| {
        section_group_rank(a.section.as_deref())
            .cmp(&section_group_rank(b.section.as_deref()))
            .then_with(|| task_order_cmp(order, a, b))
    });
}

fn apply_cap(tasks: Vec<TaskView>, cap: Option<usize>) -> (Vec<TaskView>, usize) {
    let Some(cap) = cap else {
        return (tasks, 0);
    };
    if tasks.len() <= cap {
        return (tasks, 0);
    }

    let hidden = tasks.len() - cap;
    let mut kept = tasks;
    kept.truncate(cap);
    (kept, hidden)
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, path::PathBuf};

    use pwf_models::{
        project::Project,
        task::{EffortTier, ProjectName, Tags, TaskId, TaskStatus, Timestamp},
    };

    use super::{
        ListMode, ListSection, ListTasks, ListTasksError, ListTasksOk, OrderDirection, OrderField,
        OrderSpec, PrerequisiteStatus, StatusFilter,
    };
    use crate::{
        ports::{
            project_task_location::ProjectTaskLocationClient,
            task_record::{IndexPlacement, Materialization, TaskRecord},
        },
        testing::{InMemoryStore, MIGRATOR, insert_project, project, task_record},
    };

    impl ProjectTaskLocationClient for InMemoryStore {
        type Error = Infallible;

        fn project_task_path(&self, project: &Project) -> Result<PathBuf, Self::Error> {
            Ok(PathBuf::from("/tasks").join(project.title.as_ref()))
        }
    }

    fn record(id: &str) -> TaskRecord {
        TaskRecord {
            title: id.to_string(),
            created: Some(Timestamp::new("2026-07-07")),
            source: String::new(),
            locator: format!("/notes/pwf/{id}.md"),
            placement: Some(IndexPlacement {
                index_path: "/notes/pwf/pwf.md".to_string(),
                line: 1,
            }),
            ..task_record(id)
        }
    }

    fn in_project(project: &'static str, task: TaskRecord) -> (&'static str, TaskRecord) {
        (project, task)
    }

    fn store_and_registry(tasks: &[(&'static str, TaskRecord)]) -> (InMemoryStore, Vec<Project>) {
        let mut store = InMemoryStore::default();
        let mut projects: Vec<&'static str> = tasks.iter().map(|(project, _)| *project).collect();
        projects.sort_unstable();
        projects.dedup();
        for project in &projects {
            let staged: Vec<TaskRecord> = tasks
                .iter()
                .filter(|(candidate, _)| candidate == project)
                .map(|(_, task)| task.clone())
                .collect();
            store = store.with_project(project, staged);
        }
        let registry = projects
            .iter()
            .map(|name| {
                let project_id = if *name == "pwf" { "PWF" } else { "CFG" };
                project(project_id, name)
            })
            .collect();
        (store, registry)
    }

    fn pwf_store(tasks: Vec<TaskRecord>) -> (InMemoryStore, Vec<Project>) {
        let staged: Vec<(&'static str, TaskRecord)> =
            tasks.into_iter().map(|task| ("pwf", task)).collect();
        store_and_registry(&staged)
    }

    fn prerequisite_registry() -> Vec<Project> {
        vec![project("PWF", "pwf"), project("CFG", "config-handler")]
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn selected_long_list_resolves_only_referenced_prerequisite_projects(
        pool: sqlx::SqlitePool,
    ) {
        insert_project(&pool, "PWF", "pwf", "/work/pwf", "/tasks/pwf", false).await;
        insert_project(
            &pool,
            "CFG",
            "config-handler",
            "/work/config-handler",
            "/tasks/config-handler",
            false,
        )
        .await;
        insert_project(
            &pool,
            "ALT",
            "unrelated",
            "/work/unrelated",
            "/missing/unrelated",
            false,
        )
        .await;

        let dependent = TaskRecord {
            prereq: Some("[[CFG-0014]]".to_string()),
            ..record("PWF-0001")
        };
        let prerequisite = TaskRecord {
            status: TaskStatus::Done,
            ..record("CFG-0014")
        };
        let store = InMemoryStore::default()
            .with_project("pwf", vec![dependent])
            .with_project("config-handler", vec![prerequisite]);
        let query = ListTasks {
            project_selector: Some("pwf".parse().unwrap()),
            include_prerequisite_statuses: true,
            ..default_query()
        };

        let result = super::execute(&query, &store, &pool, &store).await.unwrap();

        assert_eq!(
            result.tasks[0].prerequisite_statuses,
            [PrerequisiteStatus {
                id: TaskId::try_new("CFG-0014").unwrap(),
                status: Some(TaskStatus::Done),
            }]
        );
    }

    async fn run(
        store: &InMemoryStore,
        registry: &[Project],
        query: &ListTasks,
    ) -> Result<ListTasksOk, ListTasksError> {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory database connects");
        MIGRATOR
            .run(&pool)
            .await
            .expect("application test migrations succeed");
        for project in registry {
            insert_project(
                &pool,
                project.id.as_ref(),
                project.title.as_ref(),
                project.source.value().as_ref(),
                project.tasks.path().as_ref(),
                project.is_paused,
            )
            .await;
        }
        super::execute(query, store, &pool, store).await
    }

    fn sectioned(id: &str, section: &str) -> TaskRecord {
        TaskRecord {
            section: Some(section.to_string()),
            ..record(id)
        }
    }

    fn effort_task(id: &str, effort: &str) -> TaskRecord {
        TaskRecord {
            effort: Some(effort.to_string()),
            ..record(id)
        }
    }

    fn tagged_task(id: &str, tags: &str) -> TaskRecord {
        TaskRecord {
            tags: Some(tags.to_string()),
            ..record(id)
        }
    }

    fn requested_tags(raw: &str) -> Option<Tags> {
        Tags::from_inputs(&[raw.parse().unwrap()])
    }

    fn dated_task(id: &str, created: &str) -> TaskRecord {
        TaskRecord {
            created: Some(Timestamp::new(created)),
            ..record(id)
        }
    }

    fn default_query() -> ListTasks {
        ListTasks {
            project_selector: None,
            section: None,
            all: false,
            number: Some(100_000),
            effort: None,
            tags: None,
            order: None,
            status: None,
            include_prerequisite_statuses: false,
            mode: ListMode::Direct,
        }
    }

    fn listed_ids(result: &ListTasksOk) -> Vec<&str> {
        result.tasks.iter().map(|task| task.id.as_ref()).collect()
    }

    #[tokio::test]
    async fn long_list_projects_done_active_and_missing_prerequisite_statuses() {
        let dependent = TaskRecord {
            prereq: Some("[[CFG-0014]], [[CFG-0015]], [[CFG-9999]]".to_string()),
            ..record("PWF-0001")
        };
        let done = TaskRecord {
            status: TaskStatus::Done,
            ..record("CFG-0014")
        };
        let active = record("CFG-0015");
        let store = InMemoryStore::default()
            .with_project("pwf", vec![dependent])
            .with_project("config-handler", vec![done, active]);

        let got = run(
            &store,
            &prerequisite_registry(),
            &ListTasks {
                project_selector: Some("pwf".parse().unwrap()),
                include_prerequisite_statuses: true,
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(
            got.tasks[0].prerequisite_statuses,
            [
                PrerequisiteStatus {
                    id: TaskId::try_new("CFG-0014").unwrap(),
                    status: Some(TaskStatus::Done),
                },
                PrerequisiteStatus {
                    id: TaskId::try_new("CFG-0015").unwrap(),
                    status: Some(TaskStatus::Active),
                },
                PrerequisiteStatus {
                    id: TaskId::try_new("CFG-9999").unwrap(),
                    status: None,
                },
            ]
        );
    }

    #[tokio::test]
    async fn long_list_treats_indexed_prerequisite_without_note_as_missing() {
        let dependent = TaskRecord {
            prereq: Some("[[CFG-0014]]".to_string()),
            ..record("PWF-0001")
        };
        let missing_note = TaskRecord {
            source: String::new(),
            body: String::new(),
            placement: None,
            materialization: Materialization::MissingNote {
                expected: "/notes/config-handler/CFG-0014.md".to_string(),
            },
            ..record("CFG-0014")
        };
        let store = InMemoryStore::default()
            .with_project("pwf", vec![dependent])
            .with_project("config-handler", vec![missing_note]);

        let got = run(
            &store,
            &prerequisite_registry(),
            &ListTasks {
                project_selector: Some("pwf".parse().unwrap()),
                include_prerequisite_statuses: true,
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(
            got.tasks[0].prerequisite_statuses,
            [PrerequisiteStatus {
                id: TaskId::try_new("CFG-0014").unwrap(),
                status: None,
            }]
        );
    }

    #[tokio::test]
    async fn list_filters_active_only() {
        let done = TaskRecord {
            status: TaskStatus::Done,
            ..record("PWF-0002")
        };
        let cancelled = TaskRecord {
            status: TaskStatus::Cancelled,
            ..record("PWF-0003")
        };
        let (store, registry) = pwf_store(vec![record("PWF-0001"), done, cancelled]);

        let got = run(&store, &registry, &default_query()).await.unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0001"]);
    }

    #[tokio::test]
    async fn status_filter_defaults_to_active_and_includes_exact_or_all() {
        assert_eq!(
            StatusFilter::default(),
            StatusFilter::Exact(TaskStatus::Active)
        );
        let done = StatusFilter::Exact(TaskStatus::Done);
        assert!(done.includes(TaskStatus::Done));
        assert!(!done.includes(TaskStatus::Active));
        for status in [TaskStatus::Active, TaskStatus::Done, TaskStatus::Cancelled] {
            assert!(StatusFilter::All.includes(status));
        }
    }

    #[tokio::test]
    async fn list_status_filter_selects_exact_statuses_and_all() {
        let done = TaskRecord {
            status: TaskStatus::Done,
            placement: None,
            ..record("PWF-0002")
        };
        let cancelled = TaskRecord {
            status: TaskStatus::Cancelled,
            placement: None,
            ..record("PWF-0003")
        };
        let (store, registry) = pwf_store(vec![record("PWF-0001"), done, cancelled]);

        for (status, expected) in [
            (TaskStatus::Active, vec!["PWF-0001"]),
            (TaskStatus::Done, vec!["PWF-0002"]),
            (TaskStatus::Cancelled, vec!["PWF-0003"]),
        ] {
            let got = run(
                &store,
                &registry,
                &ListTasks {
                    status: Some(StatusFilter::Exact(status)),
                    ..default_query()
                },
            )
            .await
            .unwrap();
            assert_eq!(listed_ids(&got), expected);
        }

        let all = run(
            &store,
            &registry,
            &ListTasks {
                status: Some(StatusFilter::All),
                ..default_query()
            },
        )
        .await
        .unwrap();
        assert_eq!(listed_ids(&all), ["PWF-0003", "PWF-0002", "PWF-0001"]);
    }

    #[tokio::test]
    async fn unindexed_active_task_is_included_in_active_and_all_lists() {
        let unindexed = TaskRecord {
            placement: None,
            ..record("PWF-0002")
        };
        let (store, registry) = pwf_store(vec![record("PWF-0001"), unindexed]);

        for status_filter in [StatusFilter::Exact(TaskStatus::Active), StatusFilter::All] {
            let got = run(
                &store,
                &registry,
                &ListTasks {
                    status: Some(status_filter),
                    ..default_query()
                },
            )
            .await
            .unwrap();
            assert_eq!(listed_ids(&got), ["PWF-0002", "PWF-0001"]);
        }
    }

    #[tokio::test]
    async fn status_filter_applies_before_cap_and_hidden_count() {
        let active = TaskRecord {
            created: Some(Timestamp::new("2026-07-09")),
            ..record("PWF-0009")
        };
        let done_newer = TaskRecord {
            status: TaskStatus::Done,
            placement: None,
            created: Some(Timestamp::new("2026-07-08")),
            ..record("PWF-0002")
        };
        let done_older = TaskRecord {
            status: TaskStatus::Done,
            placement: None,
            created: Some(Timestamp::new("2026-07-07")),
            ..record("PWF-0001")
        };
        let (store, registry) = pwf_store(vec![active, done_newer, done_older]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                number: Some(1),
                status: Some(StatusFilter::Exact(TaskStatus::Done)),
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0002"]);
        assert_eq!(got.hidden, 1);
    }

    #[tokio::test]
    async fn default_scope_hides_human_future_and_low_prio_sections() {
        let (store, registry) = pwf_store(vec![
            record("PWF-0004"),
            sectioned("PWF-0003", "Human"),
            sectioned("PWF-0002", "Future"),
            sectioned("PWF-0001", "Low-prio"),
        ]);

        let got = run(&store, &registry, &default_query()).await.unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0004"]);
    }

    #[tokio::test]
    async fn raw_section_label_is_normalized_before_scoping() {
        let (store, registry) = pwf_store(vec![sectioned("PWF-0001", "Futuro")]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                section: Some(ListSection::Future),
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0001"]);
    }

    #[tokio::test]
    async fn all_scope_groups_by_section_rank() {
        let (store, registry) = pwf_store(vec![
            sectioned("FOO-0004", "Future"),
            sectioned("FOO-0003", "Human"),
            sectioned("FOO-0002", "Low-prio"),
            record("FOO-0001"),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                all: true,
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(
            listed_ids(&got),
            ["FOO-0001", "FOO-0002", "FOO-0003", "FOO-0004"]
        );
    }

    #[tokio::test]
    async fn human_scope_shows_only_human_items() {
        let (store, registry) = pwf_store(vec![
            record("PWF-0003"),
            sectioned("PWF-0002", "Human"),
            sectioned("PWF-0001", "Future"),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                section: Some(ListSection::Human),
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0002"]);
    }

    #[tokio::test]
    async fn effort_filter_matches_exact_tier_only() {
        let (store, registry) = pwf_store(vec![
            effort_task("PWF-0003", "high"),
            effort_task("PWF-0002", "medium"),
            record("PWF-0001"),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                effort: Some(EffortTier::High),
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0003"]);
    }

    #[tokio::test]
    async fn stored_effort_trims_names_and_rejects_numeric_metadata() {
        let (store, registry) = pwf_store(vec![
            effort_task("PWF-0003", " high "),
            effort_task("PWF-0002", "3"),
            effort_task("PWF-0001", "unknown"),
        ]);

        let matched = run(
            &store,
            &registry,
            &ListTasks {
                effort: Some(EffortTier::High),
                ..default_query()
            },
        )
        .await
        .unwrap();
        assert_eq!(listed_ids(&matched), ["PWF-0003"]);
    }

    #[tokio::test]
    async fn tag_filter_requires_every_requested_tag() {
        let (store, registry) = pwf_store(vec![
            tagged_task("PWF-0004", "[sqlite_tools, godot]"),
            tagged_task("PWF-0003", "[sqlite, godot]"),
            tagged_task("PWF-0002", "[sqlite]"),
            record("PWF-0001"),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                tags: requested_tags("SQLite,godot"),
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0003"]);
    }

    #[tokio::test]
    async fn corrupt_tags_fail_only_when_a_tag_filter_is_requested() {
        let (store, registry) = pwf_store(vec![tagged_task("PWF-0001", "sqlite, godot")]);
        assert!(run(&store, &registry, &default_query()).await.is_ok());

        let error = run(
            &store,
            &registry,
            &ListTasks {
                tags: requested_tags("sqlite"),
                ..default_query()
            },
        )
        .await
        .unwrap_err();

        let ListTasksError::InvalidTags { id, source } = error else {
            panic!("expected invalid tags error");
        };
        assert_eq!(id.as_ref(), "PWF-0001");
        assert_eq!(source.raw(), "sqlite, godot");
    }

    #[tokio::test]
    async fn scope_and_effort_filters_exclude_corrupt_tags_before_parsing() {
        let (store, registry) = pwf_store(vec![
            TaskRecord {
                section: Some("Human".parse().unwrap()),
                ..tagged_task("PWF-0003", "corrupt")
            },
            TaskRecord {
                effort: Some("medium".parse().unwrap()),
                ..tagged_task("PWF-0002", "also corrupt")
            },
            TaskRecord {
                effort: Some("high".parse().unwrap()),
                ..tagged_task("PWF-0001", "[sqlite]")
            },
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                effort: Some(EffortTier::High),
                tags: requested_tags("sqlite"),
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0001"]);
    }

    #[tokio::test]
    async fn tag_filter_applies_before_cap_and_hidden_count() {
        let (store, registry) = pwf_store(vec![
            record("PWF-9999"),
            tagged_task("PWF-0002", "[sqlite]"),
            tagged_task("PWF-0001", "[sqlite]"),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                number: Some(1),
                tags: requested_tags("sqlite"),
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0002"]);
        assert_eq!(got.hidden, 1);
    }

    #[tokio::test]
    async fn created_desc_is_default_and_flat_across_projects() {
        let (store, registry) = store_and_registry(&[
            in_project("pwf", dated_task("PWF-0001", "2026-01-01")),
            in_project("config-handler", dated_task("CFG-0001", "2026-03-01")),
        ]);

        let got = run(&store, &registry, &default_query()).await.unwrap();

        assert_eq!(listed_ids(&got), ["CFG-0001", "PWF-0001"]);
    }

    #[tokio::test]
    async fn created_asc_orders_oldest_first() {
        let (store, registry) = pwf_store(vec![
            dated_task("FOO-0001", "2026-01-01"),
            dated_task("FOO-0002", "2026-03-01"),
            dated_task("FOO-0003", "2026-02-01"),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                order: Some(OrderSpec {
                    field: OrderField::Created,
                    direction: OrderDirection::Asc,
                }),
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["FOO-0001", "FOO-0003", "FOO-0002"]);
    }

    #[tokio::test]
    async fn id_desc_is_flat_across_projects() {
        let (store, registry) = store_and_registry(&[
            in_project("config-handler", record("CFG-0001")),
            in_project("pwf", record("PWF-0099")),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                order: Some(OrderSpec {
                    field: OrderField::Id,
                    direction: OrderDirection::Desc,
                }),
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0099", "CFG-0001"]);
    }

    #[tokio::test]
    async fn project_route_defaults_to_project_id_order() {
        let (store, registry) = store_and_registry(&[
            in_project("pwf", record("PWF-9999")),
            in_project("config-handler", record("CFG-0001")),
            in_project("config-handler", record("CFG-0002")),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                order: None,
                mode: ListMode::ProjectRoute,
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["CFG-0002", "CFG-0001", "PWF-9999"]);
    }

    #[tokio::test]
    async fn all_uncaps_and_direct_mode_uses_the_default_cap() {
        let (store, registry) =
            pwf_store((1..=12).map(|n| record(&format!("FOO-{n:04}"))).collect());

        let all = run(
            &store,
            &registry,
            &ListTasks {
                all: true,
                number: None,
                ..default_query()
            },
        )
        .await
        .unwrap();
        assert_eq!(all.tasks.len(), 12);
        assert_eq!(all.hidden, 0);
        assert_eq!(all.status_filter, StatusFilter::All);
        assert!(all.grouped);

        let capped = run(
            &store,
            &registry,
            &ListTasks {
                number: None,
                ..default_query()
            },
        )
        .await
        .unwrap();
        assert_eq!(capped.tasks.len(), 10);
        assert_eq!(capped.hidden, 2);
        assert_eq!(capped.status_filter, StatusFilter::default());
        assert!(!capped.grouped);
    }

    #[tokio::test]
    async fn only_project_scans_just_that_project() {
        let (store, registry) = store_and_registry(&[
            in_project("pwf", record("PWF-0001")),
            in_project("config-handler", record("CFG-0001")),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                project_selector: Some("pwf".parse().unwrap()),
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0001"]);
        assert_eq!(got.project, Some(ProjectName::try_new("pwf").unwrap()));
        assert_eq!(got.project_task_path, Some(PathBuf::from("/tasks/pwf")));
    }
}
