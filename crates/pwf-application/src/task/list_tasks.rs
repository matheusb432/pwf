use pwf_models::{
    project::Project,
    task::{EffortTier, PriorityTier, TaskId, TaskSection, TaskTags},
};
use pwf_wire::{
    project::{ProjectStatusFilter, ResolveProject},
    task::{
        ListDetail, ListLayout, ListScope, ListTasks, ListedTasks, OrderDirection, OrderField,
        OrderSpec, StatusFilter, TaskView,
    },
};

use super::{blocked_by, tags, task_view};
use crate::{
    ports::{
        project_task_location::ProjectTaskLocationClient,
        task_record::{TaskRecord, TaskStore},
    },
    project::{list_projects, resolve_project},
};

/// Retains invalid requested or persisted tag text for list diagnostics.
#[derive(Debug, thiserror::Error)]
#[error("{source}")]
pub struct TagParseError {
    raw: String,
    #[source]
    source: tags::ParseTagsError,
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
            source: error,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ListTasksError {
    #[error("task read failed: {0}")]
    ReadStore(#[source] anyhow::Error),
    #[error("managed project task path read failed: {0}")]
    ReadProjectTaskPath(#[source] anyhow::Error),
    #[error("task {id} has invalid tags frontmatter: {source}")]
    InvalidTags {
        id: TaskId,
        #[source]
        source: TagParseError,
    },
    #[error(transparent)]
    InvalidTaskView(anyhow::Error),
    #[error(transparent)]
    ResolveProject(#[from] crate::project::resolve_project::ResolveProjectError),
    #[error(transparent)]
    QueryProject(anyhow::Error),
}

struct ResolvedListTasks {
    project: Option<Project>,
    scope: ListScope,
    cap: Option<usize>,
    effort: Option<EffortTier>,
    priority: Option<PriorityTier>,
    tags: Option<TaskTags>,
    order: OrderSpec,
    status_filter: StatusFilter,
    detail: ListDetail,
}

#[cqrsy::query]
pub async fn execute(
    query: &ListTasks,
    store: &impl TaskStore,
    pool: &sqlx::SqlitePool,
    task_locations: &impl ProjectTaskLocationClient,
) -> Result<ListedTasks, ListTasksError> {
    let selected = match query.project_selector.as_ref() {
        Some(selector) => Some(
            resolve_project::execute(
                ResolveProject {
                    selector: selector.clone(),
                    status: ProjectStatusFilter::ActiveOnly,
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
                .map_err(|error| ListTasksError::ReadStore(anyhow::Error::new(error)))
        })
        .transpose()?;
    let task_projects = match selected.as_ref() {
        Some(_) => Vec::new(),
        None => list_projects::execute(ProjectStatusFilter::ActiveOnly, pool)
            .await
            .map_err(|error| ListTasksError::QueryProject(anyhow::Error::new(error)))?,
    };
    let relationship_projects = if query.detail.includes_relationship_statuses() {
        list_projects::execute(ProjectStatusFilter::IncludingPaused, pool)
            .await
            .map_err(|error| ListTasksError::QueryProject(anyhow::Error::new(error)))?
    } else {
        Vec::new()
    };
    let query = resolve_query(query, selected);
    let project_task_path = query
        .project
        .as_ref()
        .map(|project| task_locations.project_task_path(project))
        .transpose()
        .map_err(|source| ListTasksError::ReadProjectTaskPath(anyhow::Error::new(source)))?;
    let mut tasks = collect_list_tasks(&query, store, &task_projects, selected_records.as_deref())?;

    tasks.retain(|task| scope_includes(query.scope, task.section.as_ref()));
    tasks.retain(|task| effort_matches(task, query.effort));
    tasks.retain(|task| priority_matches(task, query.priority));

    tasks = retain_matching_tags(tasks, query.tags.as_ref())?;

    if list_layout(query.scope) == ListLayout::BySection {
        sort_by_group_then_order(&mut tasks, query.order);
    } else {
        sort_by_order(&mut tasks, query.order);
    }

    let (mut tasks, hidden) = apply_cap(tasks, query.cap);

    if query.detail.includes_relationship_statuses() {
        populate_relationship_statuses(
            &mut tasks,
            store,
            query.project.as_ref(),
            &relationship_projects,
        );
    }

    Ok(ListedTasks {
        tasks,
        hidden,
        project: query.project.map(|project| project.title),
        project_task_path,
        status_filter: query.status_filter,
        layout: list_layout(query.scope),
        detail: query.detail,
    })
}

fn populate_relationship_statuses(
    tasks: &mut [TaskView],
    store: &impl TaskStore,
    selected_project: Option<&Project>,
    projects: &[Project],
) {
    for task in tasks {
        task.blocked_by_statuses = task.blocked_by.as_ref().map_or_else(Vec::new, |value| {
            blocked_by::statuses(value, store, selected_project, projects)
        });
    }
}

fn retain_matching_tags(
    tasks: Vec<TaskView>,
    requested: Option<&TaskTags>,
) -> Result<Vec<TaskView>, ListTasksError> {
    let Some(requested) = requested else {
        return Ok(tasks);
    };
    let mut matched = Vec::with_capacity(tasks.len());
    for task in tasks {
        let Some(raw) = task.tags.as_ref() else {
            continue;
        };
        let stored =
            tags::parse_frontmatter(raw).map_err(|source| ListTasksError::InvalidTags {
                id: task.id.clone(),
                source: source.into(),
            })?;
        if requested
            .iter()
            .all(|requested| stored.iter().any(|stored| stored == requested))
        {
            matched.push(task);
        }
    }
    Ok(matched)
}

fn resolve_query(query: &ListTasks, project: Option<Project>) -> ResolvedListTasks {
    let scope = query.scope;
    let cap = query
        .number
        .map(pwf_wire::task::TaskListLimit::get)
        .or((scope != ListScope::All).then_some(10));
    let order = query.order.unwrap_or_default();
    let status_filter = query.status.unwrap_or(if scope == ListScope::All {
        StatusFilter::All
    } else {
        StatusFilter::default()
    });

    ResolvedListTasks {
        project,
        scope,
        cap,
        effort: query.effort,
        priority: query.priority,
        tags: query.tags.clone(),
        order,
        status_filter,
        detail: query.detail,
    }
}

/// Reads and enriches listable lifecycle records from one project or every project in name order.
fn collect_list_tasks(
    query: &ResolvedListTasks,
    store: &impl TaskStore,
    projects: &[Project],
    selected_records: Option<&[TaskRecord]>,
) -> Result<Vec<TaskView>, ListTasksError> {
    let scan = query
        .project
        .as_ref()
        .map_or(projects, std::slice::from_ref);

    let mut tasks = Vec::new();
    for project in scan {
        tasks.extend(collect_project_tasks(
            query,
            store,
            project,
            selected_records,
        )?);
    }
    Ok(tasks)
}

fn collect_project_tasks(
    query: &ResolvedListTasks,
    store: &impl TaskStore,
    project: &Project,
    selected_records: Option<&[TaskRecord]>,
) -> Result<Vec<TaskView>, ListTasksError> {
    let records = if query.project.is_some() {
        selected_records.map_or_else(Vec::new, <[TaskRecord]>::to_vec)
    } else {
        store
            .list(project)
            .map_err(|error| ListTasksError::ReadStore(anyhow::Error::new(error)))?
    };
    records
        .into_iter()
        .filter(|record| query.status_filter.includes(record.status))
        .map(|record| {
            task_view::enrich(&record, project.source.value())
                .map_err(|error| ListTasksError::InvalidTaskView(anyhow::Error::new(error)))
                .map(|task| task.into_task_view(project.title.clone()))
        })
        .collect()
}

fn scope_includes(scope: ListScope, section: Option<&TaskSection>) -> bool {
    match scope {
        ListScope::Default => section.is_none(),
        ListScope::Human => section.is_some_and(|section| section.as_ref() == "Human"),
        ListScope::Future => section.is_some_and(|section| section.as_ref() == "Future"),
        ListScope::All => true,
    }
}

fn list_layout(scope: ListScope) -> ListLayout {
    if matches!(scope, ListScope::All) {
        ListLayout::BySection
    } else {
        ListLayout::Flat
    }
}

fn section_group_rank(section: Option<&TaskSection>) -> u8 {
    match section.map(AsRef::as_ref) {
        None => 0,
        Some("Low-prio") => 1,
        Some("Human") => 2,
        Some("Future") => 3,
        Some(_) => 4,
    }
}

fn effort_matches(task: &TaskView, wanted: Option<EffortTier>) -> bool {
    let Some(wanted) = wanted else { return true };
    task.effort == Some(wanted)
}

fn priority_matches(task: &TaskView, wanted: Option<PriorityTier>) -> bool {
    let Some(wanted) = wanted else { return true };
    task.priority == Some(wanted)
}

fn task_order_cmp(order: OrderSpec, a: &TaskView, b: &TaskView) -> std::cmp::Ordering {
    match order.field {
        OrderField::Created => {
            let ascending = a.created.cmp(&b.created).then_with(|| a.id.cmp(&b.id));
            match order.direction {
                OrderDirection::Asc => ascending,
                OrderDirection::Desc => ascending.reverse(),
            }
        }
        OrderField::Id => {
            let ascending =
                a.id.number()
                    .cmp(&b.id.number())
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
                .then_with(|| b.id.number().cmp(&a.id.number()))
                .then_with(|| b.id.cmp(&a.id))
        }
    }
}

fn sort_by_order(tasks: &mut [TaskView], order: OrderSpec) {
    tasks.sort_by(|a, b| task_order_cmp(order, a, b));
}

fn sort_by_group_then_order(tasks: &mut [TaskView], order: OrderSpec) {
    tasks.sort_by(|a, b| {
        section_group_rank(a.section.as_ref())
            .cmp(&section_group_rank(b.section.as_ref()))
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
    use std::{convert::Infallible, num::NonZeroUsize};

    use pwf_models::{
        project::{Project, ProjectName},
        task::{EffortTier, PriorityTier, TaskId, TaskStatus, TaskTags},
    };
    use pwf_wire::task::{
        BlockedByResolution, BlockedByStatus, ListDetail, ListLayout, ListScope, ListedTasks,
        OrderDirection, OrderField, OrderSpec, ProjectTaskPath, RawTaskTags, StatusFilter,
        TaskIndexPath, TaskListLimit, TaskNotePath,
    };

    use super::{ListTasks, ListTasksError};
    use crate::{
        ports::{
            project_task_location::ProjectTaskLocationClient,
            task_record::{IndexPlacement, Materialization, TaskRecord},
        },
        task::list_tasks,
        testing::{
            InMemoryStore, MIGRATOR, insert_project, project, stored_blocked_by, task_record,
            task_timestamp,
        },
    };

    impl ProjectTaskLocationClient for InMemoryStore {
        type Error = Infallible;

        fn project_task_path(&self, project: &Project) -> Result<ProjectTaskPath, Self::Error> {
            Ok(ProjectTaskPath::new(
                std::path::Path::new("/tasks").join(project.title.as_ref()),
            ))
        }
    }

    fn record(id: &str) -> TaskRecord {
        TaskRecord {
            title: id.to_string(),
            created_at: Some(task_timestamp("2026-07-07T12:34:56Z")),
            source: String::new(),
            locator: TaskNotePath::new(format!("/notes/foo/{id}.md").into()),
            placement: Some(IndexPlacement {
                index_path: TaskIndexPath::new("/notes/foo/foo.md".into()),
                line: NonZeroUsize::MIN,
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
            .map(|name| project(project_id_for_name(name), name))
            .collect();
        (store, registry)
    }

    fn project_id_for_name(name: &str) -> &'static str {
        if name == "foo" { "FOO" } else { "AUX" }
    }

    fn foo_store(tasks: Vec<TaskRecord>) -> (InMemoryStore, Vec<Project>) {
        let staged: Vec<(&'static str, TaskRecord)> =
            tasks.into_iter().map(|task| ("foo", task)).collect();
        store_and_registry(&staged)
    }

    fn blocked_by_registry() -> Vec<Project> {
        vec![project("FOO", "foo"), project("AUX", "companion-project")]
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn selected_long_list_resolves_blockers_from_paused_projects(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
        insert_project(
            &pool,
            "AUX",
            "companion-project",
            "/work/companion-project",
            "/tasks/companion-project",
            true,
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
            blocked_by: stored_blocked_by(&["AUX-0014"]),
            ..record("FOO-0001")
        };
        let blocking_task = TaskRecord {
            status: TaskStatus::Done,
            ..record("AUX-0014")
        };
        let store = InMemoryStore::default()
            .with_project("foo", vec![dependent])
            .with_project("companion-project", vec![blocking_task]);
        let query = ListTasks {
            project_selector: Some("foo".parse().unwrap()),
            detail: ListDetail::Detailed,
            ..default_query()
        };

        let result = list_tasks::execute(&query, &store, &pool, &store)
            .await
            .unwrap();

        assert_eq!(
            result.tasks[0].blocked_by_statuses,
            [BlockedByStatus {
                id: TaskId::try_new("AUX-0014").unwrap(),
                title: Some("AUX-0014".to_string()),
                resolution: BlockedByResolution::Found(TaskStatus::Done),
            }]
        );
    }

    async fn run(
        store: &InMemoryStore,
        registry: &[Project],
        query: &ListTasks,
    ) -> Result<ListedTasks, ListTasksError> {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        MIGRATOR.run(&pool).await.unwrap();
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
        list_tasks::execute(query, store, &pool, store).await
    }

    fn sectioned(id: &str, section: &str) -> TaskRecord {
        TaskRecord {
            section: Some(section.parse().unwrap()),
            ..record(id)
        }
    }

    fn effort_task(id: &str, effort: &str) -> TaskRecord {
        TaskRecord {
            effort: Some(effort.to_string()),
            ..record(id)
        }
    }

    fn priority_task(id: &str, priority: &str) -> TaskRecord {
        TaskRecord {
            priority: Some(priority.to_string()),
            ..record(id)
        }
    }

    fn tagged_task(id: &str, tags: &str) -> TaskRecord {
        TaskRecord {
            tags: Some(RawTaskTags::new(tags)),
            ..record(id)
        }
    }

    fn requested_tags(raw: &str) -> Option<TaskTags> {
        TaskTags::from_inputs(&[raw.parse().unwrap()])
    }

    fn dated_task(id: &str, created_date: &str) -> TaskRecord {
        TaskRecord {
            created_at: Some(task_timestamp(format!("{created_date}T00:00:00Z"))),
            ..record(id)
        }
    }

    fn default_query() -> ListTasks {
        ListTasks {
            project_selector: None,
            scope: ListScope::Default,
            number: TaskListLimit::try_new(100_000).ok(),
            effort: None,
            priority: None,
            tags: None,
            order: None,
            status: None,
            detail: ListDetail::Summary,
        }
    }

    fn listed_ids(result: &ListedTasks) -> Vec<&str> {
        result.tasks.iter().map(|task| task.id.as_ref()).collect()
    }

    async fn assert_filter_ids(
        store: &InMemoryStore,
        registry: &[Project],
        status: StatusFilter,
        expected: &[&str],
    ) {
        let got = run(
            store,
            registry,
            &ListTasks {
                status: Some(status),
                ..default_query()
            },
        )
        .await
        .unwrap();
        assert_eq!(listed_ids(&got), expected);
    }

    #[tokio::test]
    async fn long_list_projects_done_active_and_missing_blocked_by_statuses() {
        let dependent = TaskRecord {
            blocked_by: stored_blocked_by(&["AUX-0014", "AUX-0015", "AUX-9999"]),
            ..record("FOO-0001")
        };
        let done = TaskRecord {
            status: TaskStatus::Done,
            ..record("AUX-0014")
        };
        let active = record("AUX-0015");
        let store = InMemoryStore::default()
            .with_project("foo", vec![dependent])
            .with_project("companion-project", vec![done, active]);

        let got = run(
            &store,
            &blocked_by_registry(),
            &ListTasks {
                project_selector: Some("foo".parse().unwrap()),
                detail: ListDetail::Detailed,
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(
            got.tasks[0].blocked_by_statuses,
            [
                BlockedByStatus {
                    id: TaskId::try_new("AUX-0014").unwrap(),
                    title: Some("AUX-0014".to_string()),
                    resolution: BlockedByResolution::Found(TaskStatus::Done),
                },
                BlockedByStatus {
                    id: TaskId::try_new("AUX-0015").unwrap(),
                    title: Some("AUX-0015".to_string()),
                    resolution: BlockedByResolution::Found(TaskStatus::Active),
                },
                BlockedByStatus {
                    id: TaskId::try_new("AUX-9999").unwrap(),
                    title: None,
                    resolution: BlockedByResolution::Missing,
                },
            ]
        );
    }

    #[tokio::test]
    async fn long_list_treats_indexed_blocked_by_without_note_as_missing() {
        let dependent = TaskRecord {
            blocked_by: stored_blocked_by(&["AUX-0014"]),
            ..record("FOO-0001")
        };
        let missing_note = TaskRecord {
            source: String::new(),
            body: String::new(),
            placement: None,
            materialization: Materialization::MissingNote {
                expected: TaskNotePath::new("/notes/companion-project/AUX-0014.md".into()),
            },
            ..record("AUX-0014")
        };
        let store = InMemoryStore::default()
            .with_project("foo", vec![dependent])
            .with_project("companion-project", vec![missing_note]);

        let got = run(
            &store,
            &blocked_by_registry(),
            &ListTasks {
                project_selector: Some("foo".parse().unwrap()),
                detail: ListDetail::Detailed,
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(
            got.tasks[0].blocked_by_statuses,
            [BlockedByStatus {
                id: TaskId::try_new("AUX-0014").unwrap(),
                title: None,
                resolution: BlockedByResolution::Missing,
            }]
        );
    }

    #[tokio::test]
    async fn list_filters_active_only() {
        let done = TaskRecord {
            status: TaskStatus::Done,
            ..record("FOO-0002")
        };
        let cancelled = TaskRecord {
            status: TaskStatus::Cancelled,
            ..record("FOO-0003")
        };
        let (store, registry) = foo_store(vec![record("FOO-0001"), done, cancelled]);

        let got = run(&store, &registry, &default_query()).await.unwrap();

        assert_eq!(listed_ids(&got), ["FOO-0001"]);
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
        assert!(
            [TaskStatus::Active, TaskStatus::Done, TaskStatus::Cancelled]
                .into_iter()
                .all(|status| StatusFilter::All.includes(status))
        );
    }

    #[tokio::test]
    async fn list_status_filter_selects_exact_statuses_and_all() {
        let done = TaskRecord {
            status: TaskStatus::Done,
            placement: None,
            ..record("FOO-0002")
        };
        let cancelled = TaskRecord {
            status: TaskStatus::Cancelled,
            placement: None,
            ..record("FOO-0003")
        };
        let (store, registry) = foo_store(vec![record("FOO-0001"), done, cancelled]);

        assert_filter_ids(
            &store,
            &registry,
            StatusFilter::Exact(TaskStatus::Active),
            &["FOO-0001"],
        )
        .await;
        assert_filter_ids(
            &store,
            &registry,
            StatusFilter::Exact(TaskStatus::Done),
            &["FOO-0002"],
        )
        .await;
        assert_filter_ids(
            &store,
            &registry,
            StatusFilter::Exact(TaskStatus::Cancelled),
            &["FOO-0003"],
        )
        .await;

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
        assert_eq!(listed_ids(&all), ["FOO-0003", "FOO-0002", "FOO-0001"]);
    }

    #[tokio::test]
    async fn unindexed_active_task_is_included_in_active_and_all_lists() {
        let unindexed = TaskRecord {
            placement: None,
            ..record("FOO-0002")
        };
        let (store, registry) = foo_store(vec![record("FOO-0001"), unindexed]);

        assert_filter_ids(
            &store,
            &registry,
            StatusFilter::Exact(TaskStatus::Active),
            &["FOO-0002", "FOO-0001"],
        )
        .await;
        assert_filter_ids(
            &store,
            &registry,
            StatusFilter::All,
            &["FOO-0002", "FOO-0001"],
        )
        .await;
    }

    #[tokio::test]
    async fn status_filter_applies_before_cap_and_hidden_count() {
        let active = TaskRecord {
            created_at: Some(task_timestamp("2026-07-09T12:34:56Z")),
            ..record("FOO-0009")
        };
        let done_newer = TaskRecord {
            status: TaskStatus::Done,
            placement: None,
            created_at: Some(task_timestamp("2026-07-08T12:34:56Z")),
            ..record("FOO-0002")
        };
        let done_older = TaskRecord {
            status: TaskStatus::Done,
            placement: None,
            created_at: Some(task_timestamp("2026-07-07T12:34:56Z")),
            ..record("FOO-0001")
        };
        let (store, registry) = foo_store(vec![active, done_newer, done_older]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                number: TaskListLimit::try_new(1).ok(),
                status: Some(StatusFilter::Exact(TaskStatus::Done)),
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["FOO-0002"]);
        assert_eq!(got.hidden, 1);
    }

    #[tokio::test]
    async fn default_scope_hides_human_future_and_low_prio_sections() {
        let (store, registry) = foo_store(vec![
            record("FOO-0004"),
            sectioned("FOO-0003", "Human"),
            sectioned("FOO-0002", "Future"),
            sectioned("FOO-0001", "Low-prio"),
        ]);

        let got = run(&store, &registry, &default_query()).await.unwrap();

        assert_eq!(listed_ids(&got), ["FOO-0004"]);
    }

    #[tokio::test]
    async fn raw_section_label_is_normalized_before_scoping() {
        let (store, registry) = foo_store(vec![sectioned("FOO-0001", "Futuro")]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                scope: ListScope::Future,
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["FOO-0001"]);
    }

    #[tokio::test]
    async fn all_scope_groups_by_section_rank() {
        let (store, registry) = foo_store(vec![
            sectioned("FOO-0004", "Future"),
            sectioned("FOO-0003", "Human"),
            sectioned("FOO-0002", "Low-prio"),
            record("FOO-0001"),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                scope: ListScope::All,
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
        let (store, registry) = foo_store(vec![
            record("FOO-0003"),
            sectioned("FOO-0002", "Human"),
            sectioned("FOO-0001", "Future"),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                scope: ListScope::Human,
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["FOO-0002"]);
    }

    #[tokio::test]
    async fn effort_filter_matches_exact_tier_only() {
        let (store, registry) = foo_store(vec![
            effort_task("FOO-0003", "high"),
            effort_task("FOO-0002", "medium"),
            record("FOO-0001"),
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

        assert_eq!(listed_ids(&got), ["FOO-0003"]);
    }

    #[tokio::test]
    async fn priority_filter_matches_exact_tier_only() {
        let (store, registry) = foo_store(vec![
            priority_task("FOO-0003", "highest"),
            priority_task("FOO-0002", "medium"),
            record("FOO-0001"),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                priority: Some(PriorityTier::Highest),
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["FOO-0003"]);
    }

    #[tokio::test]
    async fn stored_effort_trims_valid_names() {
        let (store, registry) = foo_store(vec![effort_task("FOO-0003", " high ")]);

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
        assert_eq!(listed_ids(&matched), ["FOO-0003"]);
    }

    #[tokio::test]
    async fn invalid_stored_effort_fails_at_the_list_boundary() {
        let (store, registry) = foo_store(vec![effort_task("FOO-0002", "3")]);

        let error = run(&store, &registry, &default_query()).await.unwrap_err();

        assert!(matches!(error, ListTasksError::InvalidTaskView { .. }));
        assert!(error.to_string().contains("invalid effort value \"3\""));
    }

    #[tokio::test]
    async fn tag_filter_requires_every_requested_tag() {
        let (store, registry) = foo_store(vec![
            tagged_task("FOO-0004", "[sqlite_tools, godot]"),
            tagged_task("FOO-0003", "[sqlite, godot]"),
            tagged_task("FOO-0002", "[sqlite]"),
            record("FOO-0001"),
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

        assert_eq!(listed_ids(&got), ["FOO-0003"]);
    }

    #[tokio::test]
    async fn corrupt_tags_fail_only_when_a_tag_filter_is_requested() {
        let (store, registry) = foo_store(vec![tagged_task("FOO-0001", "sqlite, godot")]);
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

        let invalid_tags = match error {
            ListTasksError::InvalidTags { id, source } => Some((id, source)),
            _ => None,
        };
        assert!(invalid_tags.is_some());
        let (id, source) = invalid_tags.unwrap();
        assert_eq!(id.as_ref(), "FOO-0001");
        assert_eq!(source.raw(), "sqlite, godot");
    }

    #[tokio::test]
    async fn scope_and_effort_filters_exclude_corrupt_tags_before_parsing() {
        let (store, registry) = foo_store(vec![
            TaskRecord {
                section: Some("Human".parse().unwrap()),
                ..tagged_task("FOO-0003", "corrupt")
            },
            TaskRecord {
                effort: Some("medium".parse().unwrap()),
                ..tagged_task("FOO-0002", "also corrupt")
            },
            TaskRecord {
                effort: Some("high".parse().unwrap()),
                ..tagged_task("FOO-0001", "[sqlite]")
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

        assert_eq!(listed_ids(&got), ["FOO-0001"]);
    }

    #[tokio::test]
    async fn tag_filter_applies_before_cap_and_hidden_count() {
        let (store, registry) = foo_store(vec![
            record("FOO-9999"),
            tagged_task("FOO-0002", "[sqlite]"),
            tagged_task("FOO-0001", "[sqlite]"),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                number: TaskListLimit::try_new(1).ok(),
                tags: requested_tags("sqlite"),
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["FOO-0002"]);
        assert_eq!(got.hidden, 1);
    }

    #[tokio::test]
    async fn created_desc_is_default_and_flat_across_projects() {
        let (store, registry) = store_and_registry(&[
            in_project("foo", dated_task("FOO-0001", "2026-01-01")),
            in_project("companion-project", dated_task("AUX-0001", "2026-03-01")),
        ]);

        let got = run(&store, &registry, &default_query()).await.unwrap();

        assert_eq!(listed_ids(&got), ["AUX-0001", "FOO-0001"]);
    }

    #[tokio::test]
    async fn created_asc_orders_oldest_first() {
        let (store, registry) = foo_store(vec![
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
            in_project("companion-project", record("AUX-0001")),
            in_project("foo", record("FOO-0099")),
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

        assert_eq!(listed_ids(&got), ["FOO-0099", "AUX-0001"]);
    }

    #[tokio::test]
    async fn project_id_order_groups_projects_and_orders_ids_descending() {
        let (store, registry) = store_and_registry(&[
            in_project("foo", record("FOO-9999")),
            in_project("companion-project", record("AUX-0001")),
            in_project("companion-project", record("AUX-0002")),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                order: Some(OrderSpec {
                    field: OrderField::ProjectId,
                    direction: OrderDirection::Asc,
                }),
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["AUX-0002", "AUX-0001", "FOO-9999"]);
    }

    #[tokio::test]
    async fn all_uncaps_and_default_scope_uses_the_default_cap() {
        let (store, registry) =
            foo_store((1..=12).map(|n| record(&format!("FOO-{n:04}"))).collect());

        let all = run(
            &store,
            &registry,
            &ListTasks {
                scope: ListScope::All,
                number: None,
                ..default_query()
            },
        )
        .await
        .unwrap();
        assert_eq!(all.tasks.len(), 12);
        assert_eq!(all.hidden, 0);
        assert_eq!(all.status_filter, StatusFilter::All);
        assert_eq!(all.layout, ListLayout::BySection);

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
        assert_eq!(capped.layout, ListLayout::Flat);
    }

    #[tokio::test]
    async fn only_project_scans_just_that_project() {
        let (store, registry) = store_and_registry(&[
            in_project("foo", record("FOO-0001")),
            in_project("companion-project", record("AUX-0001")),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                project_selector: Some("foo".parse().unwrap()),
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["FOO-0001"]);
        assert_eq!(got.project, Some(ProjectName::try_new("foo").unwrap()));
        assert_eq!(
            got.project_task_path,
            Some(ProjectTaskPath::new("/tasks/foo".into()))
        );
    }
}
