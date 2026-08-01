use std::path::PathBuf;

use pwf_models::{
    pending_work::{EffortTier, ProjectName, Tags, WorkItemStatus},
    project::Project,
};

#[cfg(test)]
use super::dto::PrerequisiteStatus;
use super::{dto::PendingWorkItemView, prerequisite, tag_policy};
use crate::{
    pending_work::enrich::{enrich, is_open_item},
    ports::{
        pending_work_record::{PendingWorkRecord, PendingWorkStore},
        project_task_location::ProjectTaskLocationClient,
    },
    project::{
        ProjectStatusFilter,
        get_project::{self, GetProject, GetProjectError},
        list_projects::{self, ListProjects},
        resolve_project::{self, ResolveProject},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetPendingWorkOk {
    pub items: Vec<PendingWorkItemView>,
    pub hidden: usize,
    pub project: Option<ProjectName>,
    pub project_task_path: Option<PathBuf>,
    pub status_filter: StatusFilter,
    pub grouped: bool,
}

/// Selects one lifecycle status or includes every lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusFilter {
    Exact(WorkItemStatus),
    All,
}

impl StatusFilter {
    #[must_use]
    pub fn includes(self, status: WorkItemStatus) -> bool {
        match self {
            Self::Exact(expected) => expected == status,
            Self::All => true,
        }
    }
}

impl Default for StatusFilter {
    fn default() -> Self {
        Self::Exact(WorkItemStatus::Active)
    }
}

/// Selects an explicit pending-work index section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListSection {
    /// Includes only items in the normalized `Human` section.
    Human,
    /// Includes only items in the normalized `Future` section.
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
    /// Orders by the numeric item-id suffix.
    Id,
    /// Orders by project name, then by newest item id within each project.
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
    /// Primary field used to order listed items.
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
pub struct GetPendingWork {
    pub project_identifier: Option<String>,
    pub section: Option<ListSection>,
    pub all: bool,
    /// Explicit item cap. Omission uses the mode-specific default.
    pub number: Option<usize>,
    pub effort: Option<EffortTier>,
    pub tags: Vec<String>,
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

impl From<tag_policy::ParseTagsError> for TagParseError {
    fn from(error: tag_policy::ParseTagsError) -> Self {
        Self {
            raw: error.raw().to_string(),
            message: error.to_string(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GetPendingWorkError {
    #[error("pending-work read failed: {0}")]
    ReadStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("managed project task path read failed: {0}")]
    ReadProjectTaskPath(Box<dyn std::error::Error + Send + Sync>),
    #[error("item {id} has invalid tags frontmatter: {source}")]
    InvalidTags {
        id: String,
        #[source]
        source: TagParseError,
    },
    #[error("invalid requested tags: {0}")]
    InvalidRequestedTags(#[source] TagParseError),
    #[error(transparent)]
    ResolveProject(#[from] crate::project::resolve_project::ResolveProjectError),
    #[error("{0}")]
    QueryProject(#[source] Box<dyn std::error::Error + Send + Sync>),
}

struct ResolvedGetPendingWork {
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
    query: &GetPendingWork,
    store: &impl PendingWorkStore,
    pool: &sqlx::SqlitePool,
    task_locations: &impl ProjectTaskLocationClient,
) -> Result<GetPendingWorkOk, GetPendingWorkError> {
    let selected = match query.project_identifier.as_deref() {
        Some(identifier) => Some(
            resolve_project::execute(
                ResolveProject {
                    identifier: identifier.to_string(),
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
                .map_err(|error| GetPendingWorkError::ReadStore(Box::new(error)))
        })
        .transpose()?;
    let projects = match selected.as_ref() {
        Some(project) => {
            let mut projects = vec![project.clone()];
            if query.include_prerequisite_statuses {
                for id in prerequisite::referenced_project_ids(
                    selected_records
                        .iter()
                        .flatten()
                        .filter_map(|record| record.prereq.as_deref()),
                ) {
                    if projects.iter().any(|project| project.id == id) {
                        continue;
                    }
                    match get_project::execute(
                        GetProject {
                            id,
                            status: ProjectStatusFilter::ACTIVE,
                        },
                        pool,
                    )
                    .await
                    {
                        Ok(project) => projects.push(project),
                        Err(GetProjectError::ProjectNotFound { .. }) => {}
                        Err(error) => {
                            return Err(GetPendingWorkError::QueryProject(Box::new(error)));
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
        .map_err(|error| GetPendingWorkError::QueryProject(Box::new(error)))?,
    };
    execute_with_projects(
        query,
        store,
        selected,
        &projects,
        selected_records.as_deref(),
        task_locations,
    )
}

fn execute_with_projects(
    query: &GetPendingWork,
    store: &impl PendingWorkStore,
    selected: Option<Project>,
    projects: &[Project],
    selected_records: Option<&[PendingWorkRecord]>,
    task_locations: &impl ProjectTaskLocationClient,
) -> Result<GetPendingWorkOk, GetPendingWorkError> {
    let query = resolve_query(query, selected)?;
    let project_task_path = query
        .project
        .as_ref()
        .map(|project| task_locations.project_task_path(project))
        .transpose()
        .map_err(|source| GetPendingWorkError::ReadProjectTaskPath(Box::new(source)))?;
    let mut items = collect_list_items(&query, store, projects, selected_records)?;

    items.retain(|item| scope_includes(query.scope, item.section.as_deref()));
    items.retain(|item| effort_matches(item, query.effort));

    if let Some(requested) = query.tags.as_ref() {
        let mut matched = Vec::with_capacity(items.len());
        for item in items {
            let Some(raw) = item.tags.as_deref() else {
                continue;
            };
            let stored = tag_policy::parse_frontmatter(raw).map_err(|source| {
                GetPendingWorkError::InvalidTags {
                    id: item.id.clone(),
                    source: source.into(),
                }
            })?;
            if tag_policy::contains_all(&stored, requested) {
                matched.push(item);
            }
        }
        items = matched;
    }

    if scope_groups_output(query.scope) {
        sort_by_group_then_order(&mut items, query.order);
    } else {
        sort_by_order(&mut items, query.order);
    }

    let (mut items, hidden) = apply_cap(items, query.cap);

    if query.include_prerequisite_statuses {
        for item in &mut items {
            item.prerequisite_statuses = item.prereq.as_deref().map_or_else(Vec::new, |value| {
                prerequisite::statuses(value, store, projects)
            });
        }
    }

    Ok(GetPendingWorkOk {
        items,
        hidden,
        project: query.project.map(|project| project.title),
        project_task_path,
        status_filter: query.status_filter,
        grouped: scope_groups_output(query.scope),
    })
}

fn resolve_query(
    query: &GetPendingWork,
    project: Option<Project>,
) -> Result<ResolvedGetPendingWork, GetPendingWorkError> {
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
    let tags = if query.tags.is_empty() {
        None
    } else {
        Some(
            tag_policy::parse_values(&query.tags)
                .map_err(|error| GetPendingWorkError::InvalidRequestedTags(error.into()))?,
        )
    };
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

    Ok(ResolvedGetPendingWork {
        project,
        scope,
        cap,
        effort: query.effort,
        tags,
        order,
        status_filter,
        include_prerequisite_statuses: query.include_prerequisite_statuses,
    })
}

/// Reads and enriches listable lifecycle records from one project or every project in name order.
fn collect_list_items(
    query: &ResolvedGetPendingWork,
    store: &impl PendingWorkStore,
    projects: &[Project],
    selected_records: Option<&[PendingWorkRecord]>,
) -> Result<Vec<PendingWorkItemView>, GetPendingWorkError> {
    let scan: Vec<&Project> = match query.project.as_ref() {
        Some(project) => vec![project],
        None => projects.iter().collect(),
    };

    let mut items = Vec::new();
    for project in scan {
        let records = if query.project.is_some() {
            selected_records.map_or_else(Vec::new, <[PendingWorkRecord]>::to_vec)
        } else {
            store
                .list(project)
                .map_err(|error| GetPendingWorkError::ReadStore(Box::new(error)))?
        };
        for record in records {
            let listable = is_open_item(&record) || record.status != WorkItemStatus::Active;
            if !listable || !query.status_filter.includes(record.status) {
                continue;
            }
            items.push(
                enrich(&record, Some(project.source.value().as_ref()))
                    .into_pending_work_item_view(project.title.to_string()),
            );
        }
    }
    Ok(items)
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

fn effort_matches(item: &PendingWorkItemView, wanted: Option<EffortTier>) -> bool {
    let Some(wanted) = wanted else { return true };
    item.effort.as_deref().and_then(parse_effort_tier) == Some(wanted)
}

fn parse_effort_tier(raw: &str) -> Option<EffortTier> {
    raw.trim().parse().ok()
}

fn id_suffix(id: &str) -> u64 {
    id.rsplit_once('-')
        .and_then(|(_, digits)| digits.parse().ok())
        .unwrap_or(0)
}

fn created_key(item: &PendingWorkItemView) -> &str {
    item.created.as_deref().unwrap_or("")
}

fn item_order_cmp(
    order: OrderSpec,
    a: &PendingWorkItemView,
    b: &PendingWorkItemView,
) -> std::cmp::Ordering {
    if order.field == OrderField::ProjectId {
        let project_cmp = match order.direction {
            OrderDirection::Asc => a.project.cmp(&b.project),
            OrderDirection::Desc => b.project.cmp(&a.project),
        };
        return project_cmp
            .then_with(|| id_suffix(&b.id).cmp(&id_suffix(&a.id)))
            .then_with(|| b.id.cmp(&a.id));
    }

    let ascending = match order.field {
        OrderField::Created => created_key(a).cmp(created_key(b)),
        OrderField::Id => id_suffix(&a.id).cmp(&id_suffix(&b.id)),
        OrderField::ProjectId => unreachable!("handled above"),
    }
    .then_with(|| a.id.cmp(&b.id));

    match order.direction {
        OrderDirection::Asc => ascending,
        OrderDirection::Desc => ascending.reverse(),
    }
}

fn sort_by_order(items: &mut [PendingWorkItemView], order: OrderSpec) {
    items.sort_by(|a, b| item_order_cmp(order, a, b));
}

fn sort_by_group_then_order(items: &mut [PendingWorkItemView], order: OrderSpec) {
    items.sort_by(|a, b| {
        section_group_rank(a.section.as_deref())
            .cmp(&section_group_rank(b.section.as_deref()))
            .then_with(|| item_order_cmp(order, a, b))
    });
}

fn apply_cap(
    items: Vec<PendingWorkItemView>,
    cap: Option<usize>,
) -> (Vec<PendingWorkItemView>, usize) {
    let Some(cap) = cap else {
        return (items, 0);
    };
    if items.len() <= cap {
        return (items, 0);
    }

    let hidden = items.len() - cap;
    let mut kept = items;
    kept.truncate(cap);
    (kept, hidden)
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, path::PathBuf};

    use pwf_models::{
        pending_work::{EffortTier, ProjectName, Timestamp, WorkItemId, WorkItemStatus},
        project::Project,
    };

    use super::{
        GetPendingWork, GetPendingWorkError, GetPendingWorkOk, ListMode, ListSection,
        OrderDirection, OrderField, OrderSpec, PrerequisiteStatus, StatusFilter,
    };
    use crate::{
        ports::{
            pending_work_record::{IndexPlacement, Materialization, PendingWorkRecord, RecordId},
            project_task_location::ProjectTaskLocationClient,
        },
        testing::{InMemoryStore, insert_project, project},
    };

    impl ProjectTaskLocationClient for InMemoryStore {
        type Error = Infallible;

        fn project_task_path(&self, project: &Project) -> Result<PathBuf, Self::Error> {
            Ok(PathBuf::from("/tasks").join(project.title.as_ref()))
        }
    }

    fn record(id: &str) -> PendingWorkRecord {
        PendingWorkRecord {
            id: RecordId::Item(WorkItemId::try_new(id).unwrap()),
            title: id.to_string(),
            status: WorkItemStatus::Active,
            created: Some(Timestamp::new("2026-07-07")),
            completed: None,
            commits: None,
            tags: None,
            effort: None,
            prereq: None,
            section: None,
            body: String::new(),
            source: String::new(),
            locator: format!("/notes/pwf/{id}.md"),
            placement: Some(IndexPlacement {
                index_path: "/notes/pwf/pwf.md".to_string(),
                line: 1,
            }),
            materialization: Materialization::NoteFile,
        }
    }

    fn in_project(
        project: &'static str,
        item: PendingWorkRecord,
    ) -> (&'static str, PendingWorkRecord) {
        (project, item)
    }

    fn store_and_registry(
        items: &[(&'static str, PendingWorkRecord)],
    ) -> (InMemoryStore, Vec<Project>) {
        let mut store = InMemoryStore::default();
        let mut projects: Vec<&'static str> = items.iter().map(|(project, _)| *project).collect();
        projects.sort_unstable();
        projects.dedup();
        for project in &projects {
            let staged: Vec<PendingWorkRecord> = items
                .iter()
                .filter(|(candidate, _)| candidate == project)
                .map(|(_, item)| item.clone())
                .collect();
            store = store.with_project(project, staged);
        }
        let registry = projects
            .iter()
            .map(|name| project(if *name == "pwf" { "PWF" } else { "CFG" }, name))
            .collect();
        (store, registry)
    }

    fn pwf_store(items: Vec<PendingWorkRecord>) -> (InMemoryStore, Vec<Project>) {
        let staged: Vec<(&'static str, PendingWorkRecord)> =
            items.into_iter().map(|item| ("pwf", item)).collect();
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

        let dependent = PendingWorkRecord {
            prereq: Some("[[CFG-0014]]".to_string()),
            ..record("PWF-0001")
        };
        let prerequisite = PendingWorkRecord {
            status: WorkItemStatus::Done,
            ..record("CFG-0014")
        };
        let store = InMemoryStore::default()
            .with_project("pwf", vec![dependent])
            .with_project("config-handler", vec![prerequisite]);
        let query = GetPendingWork {
            project_identifier: Some("pwf".to_string()),
            include_prerequisite_statuses: true,
            ..default_query()
        };

        let result = super::execute(&query, &store, &pool, &store).await.unwrap();

        assert_eq!(
            result.items[0].prerequisite_statuses,
            [PrerequisiteStatus {
                id: WorkItemId::try_new("CFG-0014").unwrap(),
                status: Some(WorkItemStatus::Done),
            }]
        );
    }

    fn run(
        store: &InMemoryStore,
        registry: &[Project],
        query: &GetPendingWork,
    ) -> Result<GetPendingWorkOk, GetPendingWorkError> {
        let selected = query.project_identifier.as_deref().and_then(|identifier| {
            registry
                .iter()
                .find(|project| {
                    project.title.as_ref() == identifier || project.id.as_ref() == identifier
                })
                .cloned()
        });
        let selected_records = selected
            .as_ref()
            .map(|project| store.items(project.title.as_ref()));
        super::execute_with_projects(
            query,
            store,
            selected,
            registry,
            selected_records.as_deref(),
            store,
        )
    }

    fn sectioned(id: &str, section: &str) -> PendingWorkRecord {
        PendingWorkRecord {
            section: Some(section.to_string()),
            ..record(id)
        }
    }

    fn effort_item(id: &str, effort: &str) -> PendingWorkRecord {
        PendingWorkRecord {
            effort: Some(effort.to_string()),
            ..record(id)
        }
    }

    fn tagged_item(id: &str, tags: &str) -> PendingWorkRecord {
        PendingWorkRecord {
            tags: Some(tags.to_string()),
            ..record(id)
        }
    }

    fn dated_item(id: &str, created: &str) -> PendingWorkRecord {
        PendingWorkRecord {
            created: Some(Timestamp::new(created)),
            ..record(id)
        }
    }

    fn default_query() -> GetPendingWork {
        GetPendingWork {
            project_identifier: None,
            section: None,
            all: false,
            number: Some(100_000),
            effort: None,
            tags: Vec::new(),
            order: None,
            status: None,
            include_prerequisite_statuses: false,
            mode: ListMode::Direct,
        }
    }

    fn listed_ids(result: &GetPendingWorkOk) -> Vec<&str> {
        result.items.iter().map(|item| item.id.as_str()).collect()
    }

    #[test]
    fn long_list_projects_done_active_and_missing_prerequisite_statuses() {
        let dependent = PendingWorkRecord {
            prereq: Some("[[CFG-0014]], [[CFG-0015]], [[CFG-9999]]".to_string()),
            ..record("PWF-0001")
        };
        let done = PendingWorkRecord {
            status: WorkItemStatus::Done,
            ..record("CFG-0014")
        };
        let active = record("CFG-0015");
        let store = InMemoryStore::default()
            .with_project("pwf", vec![dependent])
            .with_project("config-handler", vec![done, active]);

        let got = run(
            &store,
            &prerequisite_registry(),
            &GetPendingWork {
                project_identifier: Some("pwf".to_string()),
                include_prerequisite_statuses: true,
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(
            got.items[0].prerequisite_statuses,
            [
                PrerequisiteStatus {
                    id: WorkItemId::try_new("CFG-0014").unwrap(),
                    status: Some(WorkItemStatus::Done),
                },
                PrerequisiteStatus {
                    id: WorkItemId::try_new("CFG-0015").unwrap(),
                    status: Some(WorkItemStatus::Active),
                },
                PrerequisiteStatus {
                    id: WorkItemId::try_new("CFG-9999").unwrap(),
                    status: None,
                },
            ]
        );
    }

    #[test]
    fn long_list_treats_indexed_prerequisite_without_note_as_missing() {
        let dependent = PendingWorkRecord {
            prereq: Some("[[CFG-0014]]".to_string()),
            ..record("PWF-0001")
        };
        let missing_note = PendingWorkRecord {
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
            &GetPendingWork {
                project_identifier: Some("pwf".to_string()),
                include_prerequisite_statuses: true,
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(
            got.items[0].prerequisite_statuses,
            [PrerequisiteStatus {
                id: WorkItemId::try_new("CFG-0014").unwrap(),
                status: None,
            }]
        );
    }

    #[test]
    fn list_filters_active_only() {
        let done = PendingWorkRecord {
            status: WorkItemStatus::Done,
            ..record("PWF-0002")
        };
        let cancelled = PendingWorkRecord {
            status: WorkItemStatus::Cancelled,
            ..record("PWF-0003")
        };
        let (store, registry) = pwf_store(vec![record("PWF-0001"), done, cancelled]);

        let got = run(&store, &registry, &default_query()).unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0001"]);
    }

    #[test]
    fn status_filter_defaults_to_active_and_includes_exact_or_all() {
        assert_eq!(
            StatusFilter::default(),
            StatusFilter::Exact(WorkItemStatus::Active)
        );
        let done = StatusFilter::Exact(WorkItemStatus::Done);
        assert!(done.includes(WorkItemStatus::Done));
        assert!(!done.includes(WorkItemStatus::Active));
        for status in [
            WorkItemStatus::Active,
            WorkItemStatus::Done,
            WorkItemStatus::Cancelled,
        ] {
            assert!(StatusFilter::All.includes(status));
        }
    }

    #[test]
    fn list_status_filter_selects_exact_statuses_and_all() {
        let done = PendingWorkRecord {
            status: WorkItemStatus::Done,
            placement: None,
            ..record("PWF-0002")
        };
        let cancelled = PendingWorkRecord {
            status: WorkItemStatus::Cancelled,
            placement: None,
            ..record("PWF-0003")
        };
        let (store, registry) = pwf_store(vec![record("PWF-0001"), done, cancelled]);

        for (status, expected) in [
            (WorkItemStatus::Active, vec!["PWF-0001"]),
            (WorkItemStatus::Done, vec!["PWF-0002"]),
            (WorkItemStatus::Cancelled, vec!["PWF-0003"]),
        ] {
            let got = run(
                &store,
                &registry,
                &GetPendingWork {
                    status: Some(StatusFilter::Exact(status)),
                    ..default_query()
                },
            )
            .unwrap();
            assert_eq!(listed_ids(&got), expected);
        }

        let all = run(
            &store,
            &registry,
            &GetPendingWork {
                status: Some(StatusFilter::All),
                ..default_query()
            },
        )
        .unwrap();
        assert_eq!(listed_ids(&all), ["PWF-0003", "PWF-0002", "PWF-0001"]);
    }

    #[test]
    fn active_orphan_is_hidden_from_active_and_all_lists() {
        let orphan = PendingWorkRecord {
            placement: None,
            ..record("PWF-0002")
        };
        let (store, registry) = pwf_store(vec![record("PWF-0001"), orphan]);

        for status_filter in [
            StatusFilter::Exact(WorkItemStatus::Active),
            StatusFilter::All,
        ] {
            let got = run(
                &store,
                &registry,
                &GetPendingWork {
                    status: Some(status_filter),
                    ..default_query()
                },
            )
            .unwrap();
            assert_eq!(listed_ids(&got), ["PWF-0001"]);
        }
    }

    #[test]
    fn status_filter_applies_before_cap_and_hidden_count() {
        let active = PendingWorkRecord {
            created: Some(Timestamp::new("2026-07-09")),
            ..record("PWF-0009")
        };
        let done_newer = PendingWorkRecord {
            status: WorkItemStatus::Done,
            placement: None,
            created: Some(Timestamp::new("2026-07-08")),
            ..record("PWF-0002")
        };
        let done_older = PendingWorkRecord {
            status: WorkItemStatus::Done,
            placement: None,
            created: Some(Timestamp::new("2026-07-07")),
            ..record("PWF-0001")
        };
        let (store, registry) = pwf_store(vec![active, done_newer, done_older]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                number: Some(1),
                status: Some(StatusFilter::Exact(WorkItemStatus::Done)),
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0002"]);
        assert_eq!(got.hidden, 1);
    }

    #[test]
    fn default_scope_hides_human_future_and_low_prio_sections() {
        let (store, registry) = pwf_store(vec![
            record("PWF-0004"),
            sectioned("PWF-0003", "Human"),
            sectioned("PWF-0002", "Future"),
            sectioned("PWF-0001", "Low-prio"),
        ]);

        let got = run(&store, &registry, &default_query()).unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0004"]);
    }

    #[test]
    fn raw_section_label_is_normalized_before_scoping() {
        let (store, registry) = pwf_store(vec![sectioned("PWF-0001", "Futuro")]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                section: Some(ListSection::Future),
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0001"]);
    }

    #[test]
    fn all_scope_groups_by_section_rank() {
        let (store, registry) = pwf_store(vec![
            sectioned("FOO-0004", "Future"),
            sectioned("FOO-0003", "Human"),
            sectioned("FOO-0002", "Low-prio"),
            record("FOO-0001"),
        ]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                all: true,
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(
            listed_ids(&got),
            ["FOO-0001", "FOO-0002", "FOO-0003", "FOO-0004"]
        );
    }

    #[test]
    fn human_scope_shows_only_human_items() {
        let (store, registry) = pwf_store(vec![
            record("PWF-0003"),
            sectioned("PWF-0002", "Human"),
            sectioned("PWF-0001", "Future"),
        ]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                section: Some(ListSection::Human),
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0002"]);
    }

    #[test]
    fn effort_filter_matches_exact_tier_only() {
        let (store, registry) = pwf_store(vec![
            effort_item("PWF-0003", "high"),
            effort_item("PWF-0002", "medium"),
            record("PWF-0001"),
        ]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                effort: Some(EffortTier::High),
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0003"]);
    }

    #[test]
    fn stored_effort_trims_names_and_rejects_numeric_metadata() {
        let (store, registry) = pwf_store(vec![
            effort_item("PWF-0003", " high "),
            effort_item("PWF-0002", "3"),
            effort_item("PWF-0001", "unknown"),
        ]);

        let matched = run(
            &store,
            &registry,
            &GetPendingWork {
                effort: Some(EffortTier::High),
                ..default_query()
            },
        )
        .unwrap();
        assert_eq!(listed_ids(&matched), ["PWF-0003"]);
    }

    #[test]
    fn tag_filter_requires_every_requested_tag() {
        let (store, registry) = pwf_store(vec![
            tagged_item("PWF-0004", "[sqlite_tools, godot]"),
            tagged_item("PWF-0003", "[sqlite, godot]"),
            tagged_item("PWF-0002", "[sqlite]"),
            record("PWF-0001"),
        ]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                tags: vec!["SQLite,godot".to_string()],
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0003"]);
    }

    #[test]
    fn corrupt_tags_fail_only_when_a_tag_filter_is_requested() {
        let (store, registry) = pwf_store(vec![tagged_item("PWF-0001", "sqlite, godot")]);
        assert!(run(&store, &registry, &default_query()).is_ok());

        let error = run(
            &store,
            &registry,
            &GetPendingWork {
                tags: vec!["sqlite".to_string()],
                ..default_query()
            },
        )
        .unwrap_err();

        let GetPendingWorkError::InvalidTags { id, source } = error else {
            panic!("expected invalid tags error");
        };
        assert_eq!(id, "PWF-0001");
        assert_eq!(source.raw(), "sqlite, godot");
    }

    #[test]
    fn scope_and_effort_filters_exclude_corrupt_tags_before_parsing() {
        let (store, registry) = pwf_store(vec![
            PendingWorkRecord {
                section: Some("Human".to_string()),
                ..tagged_item("PWF-0003", "corrupt")
            },
            PendingWorkRecord {
                effort: Some("medium".to_string()),
                ..tagged_item("PWF-0002", "also corrupt")
            },
            PendingWorkRecord {
                effort: Some("high".to_string()),
                ..tagged_item("PWF-0001", "[sqlite]")
            },
        ]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                effort: Some(EffortTier::High),
                tags: vec!["sqlite".to_string()],
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0001"]);
    }

    #[test]
    fn tag_filter_applies_before_cap_and_hidden_count() {
        let (store, registry) = pwf_store(vec![
            record("PWF-9999"),
            tagged_item("PWF-0002", "[sqlite]"),
            tagged_item("PWF-0001", "[sqlite]"),
        ]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                number: Some(1),
                tags: vec!["sqlite".to_string()],
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0002"]);
        assert_eq!(got.hidden, 1);
    }

    #[test]
    fn created_desc_is_default_and_flat_across_projects() {
        let (store, registry) = store_and_registry(&[
            in_project("pwf", dated_item("PWF-0001", "2026-01-01")),
            in_project("config-handler", dated_item("CFG-0001", "2026-03-01")),
        ]);

        let got = run(&store, &registry, &default_query()).unwrap();

        assert_eq!(listed_ids(&got), ["CFG-0001", "PWF-0001"]);
    }

    #[test]
    fn created_asc_orders_oldest_first() {
        let (store, registry) = pwf_store(vec![
            dated_item("FOO-0001", "2026-01-01"),
            dated_item("FOO-0002", "2026-03-01"),
            dated_item("FOO-0003", "2026-02-01"),
        ]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                order: Some(OrderSpec {
                    field: OrderField::Created,
                    direction: OrderDirection::Asc,
                }),
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["FOO-0001", "FOO-0003", "FOO-0002"]);
    }

    #[test]
    fn id_desc_is_flat_across_projects() {
        let (store, registry) = store_and_registry(&[
            in_project("config-handler", record("CFG-0001")),
            in_project("pwf", record("PWF-0099")),
        ]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                order: Some(OrderSpec {
                    field: OrderField::Id,
                    direction: OrderDirection::Desc,
                }),
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0099", "CFG-0001"]);
    }

    #[test]
    fn project_route_defaults_to_project_id_order() {
        let (store, registry) = store_and_registry(&[
            in_project("pwf", record("PWF-9999")),
            in_project("config-handler", record("CFG-0001")),
            in_project("config-handler", record("CFG-0002")),
        ]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                order: None,
                mode: ListMode::ProjectRoute,
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["CFG-0002", "CFG-0001", "PWF-9999"]);
    }

    #[test]
    fn all_uncaps_and_direct_mode_uses_the_default_cap() {
        let (store, registry) =
            pwf_store((1..=12).map(|n| record(&format!("FOO-{n:04}"))).collect());

        let all = run(
            &store,
            &registry,
            &GetPendingWork {
                all: true,
                number: None,
                ..default_query()
            },
        )
        .unwrap();
        assert_eq!(all.items.len(), 12);
        assert_eq!(all.hidden, 0);
        assert_eq!(all.status_filter, StatusFilter::All);
        assert!(all.grouped);

        let capped = run(
            &store,
            &registry,
            &GetPendingWork {
                number: None,
                ..default_query()
            },
        )
        .unwrap();
        assert_eq!(capped.items.len(), 10);
        assert_eq!(capped.hidden, 2);
        assert_eq!(capped.status_filter, StatusFilter::default());
        assert!(!capped.grouped);
    }

    #[test]
    fn inline_legacy_records_list_with_project_scoped_ids() {
        let inline = PendingWorkRecord {
            id: RecordId::Inline(1),
            title: "legacy task".to_string(),
            body: "do the legacy thing".to_string(),
            source: "do the legacy thing".to_string(),
            created: None,
            materialization: Materialization::InlineLegacy,
            ..record("PWF-0001")
        };
        let (store, registry) = pwf_store(vec![record("PWF-0002"), inline]);

        let got = run(&store, &registry, &default_query()).unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0002", "pwf:1"]);
        let legacy = &got.items[1];
        assert_eq!(legacy.format, "legacy");
        assert_eq!(legacy.item_file, None);
        assert_eq!(legacy.session, "legacy task");
    }

    #[test]
    fn only_project_scans_just_that_project() {
        let (store, registry) = store_and_registry(&[
            in_project("pwf", record("PWF-0001")),
            in_project("config-handler", record("CFG-0001")),
        ]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                project_identifier: Some("pwf".to_string()),
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0001"]);
        assert_eq!(got.project, Some(ProjectName::try_new("pwf").unwrap()));
        assert_eq!(got.project_task_path, Some(PathBuf::from("/tasks/pwf")));
    }
}
