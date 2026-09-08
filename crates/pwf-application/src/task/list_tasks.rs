mod snapshots;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use pwf_models::{
    project::Project,
    settings::UserSettings,
    task::{
        EffortTier, PriorityTier, TaskId, TaskSection, TaskTags,
        order::{OrderDirection, OrderField, OrderSpec},
    },
};
use pwf_wire::{
    pagination::CursorPage,
    project::{ProjectStatusFilter, ResolveProject},
    task::{
        ListDetail, ListLayout, ListScope, ListTasks, ListedTask, ListedTasks, StatusFilter,
        TaskPageSize, TaskPageToken,
    },
};
use serde::{Deserialize, Serialize};
pub use snapshots::ListTasksSnapshots;

use super::{blocked_by, tags, task_view};
use crate::{
    ports::{
        project_task_location::ProjectTaskLocationClient,
        task_vault::TaskVault,
        user_settings::{UserSettingsLoadError, UserSettingsReader},
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
    #[error(transparent)]
    Settings(#[from] UserSettingsLoadError),
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
    #[error("invalid page token: {reason}")]
    InvalidPageToken { reason: &'static str },
    #[error(transparent)]
    EncodePageToken(anyhow::Error),
    #[error("task-list snapshot storage is unavailable")]
    SnapshotUnavailable,
}

struct ResolvedListTasks {
    project: Option<Project>,
    scope: ListScope,
    cap: Option<usize>,
    effort: Option<EffortTier>,
    priority: Option<PriorityTier>,
    tags: Option<TaskTags>,
    order: OrderSpec,
    default_priority: PriorityTier,
    status_filter: StatusFilter,
    detail: ListDetail,
    page_size: Option<TaskPageSize>,
    page_token: Option<TaskPageToken>,
}

#[derive(Debug, PartialEq, Eq)]
struct TaskListPage {
    page: CursorPage<ListedTask, TaskPageToken>,
    /// Tasks excluded by the list cap before pagination.
    hidden: usize,
}

#[cqrsy::query]
pub async fn execute(
    query: &ListTasks,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
    task_locations: &impl ProjectTaskLocationClient,
    snapshots: &ListTasksSnapshots,
    settings_reader: &impl UserSettingsReader,
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
    let settings = settings_reader.load()?;
    let query = resolve_query(query, selected, settings);
    let binding = page_binding(&query);
    let cursor = query
        .page_token
        .as_ref()
        .map(decode_page_token)
        .transpose()?;
    if let Some(cursor) = &cursor {
        validate_page_cursor(cursor, &binding)?;
    }
    let relationship_projects = if query.detail.includes_relationship_statuses() {
        list_projects::execute(ProjectStatusFilter::IncludingPaused, pool)
            .await
            .map_err(|error| ListTasksError::QueryProject(anyhow::Error::new(error)))?
    } else {
        Vec::new()
    };
    let project_task_path = query
        .project
        .as_ref()
        .map(|project| task_locations.project_task_path(project))
        .transpose()
        .map_err(|source| ListTasksError::ReadProjectTaskPath(anyhow::Error::new(source)))?;
    let mut result =
        collect_page(&query, cursor.as_ref(), &binding, store, pool, snapshots).await?;

    if query.detail.includes_relationship_statuses() {
        populate_relationship_statuses(
            &mut result.page.items,
            store,
            query.project.as_ref(),
            &relationship_projects,
        );
    }

    Ok(ListedTasks {
        tasks: result.page.items,
        hidden: result.hidden,
        project: query.project.map(|project| project.title),
        project_task_path,
        status_filter: query.status_filter,
        layout: list_layout(&query.scope),
        detail: query.detail,
        next_page_token: result.page.next_key,
    })
}

async fn collect_page(
    query: &ResolvedListTasks,
    cursor: Option<&PageCursor>,
    binding: &str,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
    snapshots: &ListTasksSnapshots,
) -> Result<TaskListPage, ListTasksError> {
    if let Some(cursor) = cursor.filter(|cursor| cursor.snapshot.is_some()) {
        return snapshots.page(cursor, query.page_size);
    }
    let projects = if query.project.is_some() {
        Vec::new()
    } else {
        list_projects::execute(ProjectStatusFilter::ActiveOnly, pool)
            .await
            .map_err(|error| ListTasksError::QueryProject(anyhow::Error::new(error)))?
    };
    let tasks = materialize_tasks(query, store, &projects)?;
    let (tasks, hidden) = apply_cap(tasks, query.cap);
    if cursor.is_none() {
        snapshots.first_page(tasks, hidden, query.page_size, binding)
    } else {
        let page = apply_page(
            tasks,
            query.page_size,
            cursor.map(|cursor| &cursor.after),
            binding,
        )?;
        Ok(TaskListPage { page, hidden })
    }
}

fn materialize_tasks(
    query: &ResolvedListTasks,
    store: &impl TaskVault,
    projects: &[Project],
) -> Result<Vec<ListedTask>, ListTasksError> {
    let mut tasks = collect_list_tasks(query, store, projects)?;
    for task in &mut tasks {
        task.priority = Some(task.priority.unwrap_or(query.default_priority));
    }
    tasks.retain(|task| {
        scope_includes(&query.scope, task.section.as_ref())
            && effort_matches(task, query.effort)
            && priority_matches(task, query.priority)
    });
    tasks = retain_matching_tags(tasks, query.tags.as_ref())?;
    if list_layout(&query.scope) == ListLayout::BySection {
        sort_by_group_then_order(&mut tasks, query.order);
    } else {
        sort_by_order(&mut tasks, query.order);
    }
    Ok(tasks)
}

fn populate_relationship_statuses(
    tasks: &mut [ListedTask],
    store: &impl TaskVault,
    selected_project: Option<&Project>,
    projects: &[Project],
) {
    for task in tasks {
        let Some(details) = task.details.as_mut() else {
            continue;
        };
        details.blocked_by_statuses = details.blocked_by.as_ref().map_or_else(Vec::new, |value| {
            blocked_by::statuses(value, store, selected_project, projects)
        });
    }
}

fn retain_matching_tags(
    tasks: Vec<ListedTask>,
    requested: Option<&TaskTags>,
) -> Result<Vec<ListedTask>, ListTasksError> {
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

fn resolve_query(
    query: &ListTasks,
    project: Option<Project>,
    settings: UserSettings,
) -> ResolvedListTasks {
    let scope = query.scope.clone();
    let cap = query
        .number
        .map(pwf_wire::task::TaskListLimit::get)
        .or((scope != ListScope::All).then_some(10));
    let order = query.order.unwrap_or(settings.default_sort_order());
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
        default_priority: settings.default_priority(),
        status_filter,
        detail: query.detail,
        page_size: query.page_size,
        page_token: query.page_token.clone(),
    }
}

#[derive(Debug)]
struct PageCursor {
    binding: String,
    after: TaskId,
    snapshot: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct EncodedPageCursor {
    version: u8,
    binding: String,
    after: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    snapshot: Option<String>,
}

fn validate_page_cursor(cursor: &PageCursor, binding: &str) -> Result<(), ListTasksError> {
    if cursor.binding != binding {
        return Err(ListTasksError::InvalidPageToken {
            reason: "filters or ordering changed",
        });
    }
    Ok(())
}

fn page_binding(query: &ResolvedListTasks) -> String {
    let mut hasher = blake3::Hasher::new();
    let project = query
        .project
        .as_ref()
        .map_or("<all>", |project| project.id.as_ref());
    digest_field(&mut hasher, "project", project);
    let scope_name = list_scope_name(&query.scope);
    digest_field(&mut hasher, "scope", &scope_name);
    digest_field(
        &mut hasher,
        "cap",
        &query
            .cap
            .map_or_else(|| "all".to_string(), |cap| cap.to_string()),
    );
    digest_optional_field(
        &mut hasher,
        "effort",
        query.effort.as_ref().map(AsRef::as_ref),
    );
    digest_field(
        &mut hasher,
        "default_priority",
        query.default_priority.as_ref(),
    );
    digest_optional_field(
        &mut hasher,
        "priority",
        query.priority.as_ref().map(AsRef::as_ref),
    );
    if let Some(tags) = &query.tags {
        for tag in tags.iter() {
            digest_field(&mut hasher, "tag", tag.as_ref());
        }
    }
    digest_field(&mut hasher, "order_field", query.order.field.as_ref());
    digest_field(
        &mut hasher,
        "order_direction",
        query.order.direction.as_ref(),
    );
    digest_field(
        &mut hasher,
        "status",
        status_filter_name(query.status_filter),
    );
    digest_field(&mut hasher, "detail", list_detail_name(query.detail));
    digest_field(
        &mut hasher,
        "page_size",
        &query
            .page_size
            .map_or_else(|| "unpaged".to_string(), |size| size.get().to_string()),
    );
    hasher.finalize().to_hex().to_string()
}

fn digest_optional_field(hasher: &mut blake3::Hasher, name: &str, value: Option<&str>) {
    digest_field(hasher, name, value.unwrap_or("<absent>"));
}

fn digest_field(hasher: &mut blake3::Hasher, name: &str, value: &str) {
    hasher.update(&digest_length(name.len()));
    hasher.update(name.as_bytes());
    hasher.update(&digest_length(value.len()));
    hasher.update(value.as_bytes());
}

fn digest_length(length: usize) -> [u8; 8] {
    u64::try_from(length).unwrap_or(u64::MAX).to_le_bytes()
}

fn apply_page(
    tasks: Vec<ListedTask>,
    page_size: Option<TaskPageSize>,
    after: Option<&TaskId>,
    binding: &str,
) -> Result<CursorPage<ListedTask, TaskPageToken>, ListTasksError> {
    let Some(page_size) = page_size else {
        return Ok(CursorPage {
            items: tasks,
            next_key: None,
        });
    };
    let start = after.map_or(Ok(0), |after| {
        tasks
            .iter()
            .position(|task| task.id == *after)
            .map(|index| index + 1)
            .ok_or(ListTasksError::InvalidPageToken {
                reason: "cursor task is no longer present",
            })
    })?;
    let end = start.saturating_add(page_size.get()).min(tasks.len());
    let next_page_token = if end < tasks.len() {
        tasks
            .get(end - 1)
            .map(|task| encode_page_token(binding, &task.id, None))
            .transpose()?
    } else {
        None
    };
    Ok(CursorPage {
        items: tasks.into_iter().skip(start).take(end - start).collect(),
        next_key: next_page_token,
    })
}

fn encode_page_token(
    binding: &str,
    after: &TaskId,
    snapshot: Option<&str>,
) -> Result<TaskPageToken, ListTasksError> {
    let payload = serde_json::to_vec(&EncodedPageCursor {
        version: if snapshot.is_some() { 2 } else { 1 },
        binding: binding.to_string(),
        after: after.to_string(),
        snapshot: snapshot.map(str::to_string),
    })
    .map_err(|error| ListTasksError::EncodePageToken(anyhow::Error::new(error)))?;
    TaskPageToken::try_new(URL_SAFE_NO_PAD.encode(payload))
        .map_err(|error| ListTasksError::EncodePageToken(anyhow::Error::new(error)))
}

fn decode_page_token(token: &TaskPageToken) -> Result<PageCursor, ListTasksError> {
    let payload =
        URL_SAFE_NO_PAD
            .decode(token.as_ref())
            .map_err(|_| ListTasksError::InvalidPageToken {
                reason: "malformed encoding",
            })?;
    let cursor: EncodedPageCursor =
        serde_json::from_slice(&payload).map_err(|_| ListTasksError::InvalidPageToken {
            reason: "malformed payload",
        })?;
    if !matches!((cursor.version, &cursor.snapshot), (1, None) | (2, Some(_))) {
        return Err(ListTasksError::InvalidPageToken {
            reason: "unsupported version",
        });
    }
    let after = TaskId::try_new(cursor.after).map_err(|_| ListTasksError::InvalidPageToken {
        reason: "invalid cursor task",
    })?;
    Ok(PageCursor {
        binding: cursor.binding,
        after,
        snapshot: cursor.snapshot,
    })
}

fn list_scope_name(scope: &ListScope) -> String {
    match scope {
        ListScope::Default => "default".to_string(),
        ListScope::Section(section) => format!("section:{}", section.case_insensitive_key()),
        ListScope::All => "all".to_string(),
    }
}

fn status_filter_name(status: StatusFilter) -> &'static str {
    match status {
        StatusFilter::Exact(pwf_models::task::TaskStatus::Active) => "active",
        StatusFilter::Exact(pwf_models::task::TaskStatus::Done) => "done",
        StatusFilter::Exact(pwf_models::task::TaskStatus::Cancelled) => "cancelled",
        StatusFilter::All => "all",
    }
}

fn list_detail_name(detail: ListDetail) -> &'static str {
    match detail {
        ListDetail::Summary => "summary",
        ListDetail::Detailed => "detailed",
    }
}

/// Reads and enriches listable lifecycle records from one project or every project in name order.
fn collect_list_tasks(
    query: &ResolvedListTasks,
    store: &impl TaskVault,
    projects: &[Project],
) -> Result<Vec<ListedTask>, ListTasksError> {
    let scan = query
        .project
        .as_ref()
        .map_or(projects, std::slice::from_ref);

    let mut tasks = Vec::new();
    for project in scan {
        tasks.extend(collect_project_tasks(query, store, project)?);
    }
    Ok(tasks)
}

fn collect_project_tasks(
    query: &ResolvedListTasks,
    store: &impl TaskVault,
    project: &Project,
) -> Result<Vec<ListedTask>, ListTasksError> {
    if query.detail == ListDetail::Summary {
        return store
            .list_task_summaries(project)
            .map_err(|error| ListTasksError::ReadStore(anyhow::Error::new(error)))?
            .into_iter()
            .filter(|record| query.status_filter.includes(record.status))
            .map(|record| {
                task_view::summarize(record, project.title.clone())
                    .map_err(|error| ListTasksError::InvalidTaskView(anyhow::Error::new(error)))
            })
            .collect();
    }
    let records = store
        .list_tasks(project)
        .map_err(|error| ListTasksError::ReadStore(anyhow::Error::new(error)))?;
    records
        .into_iter()
        .filter(|record| query.status_filter.includes(record.status))
        .map(|record| {
            task_view::enrich(
                &record,
                project
                    .source
                    .as_ref()
                    .map(pwf_models::project::ProjectSource::value),
            )
            .map_err(|error| ListTasksError::InvalidTaskView(anyhow::Error::new(error)))
            .map(|task| task.into_task_view(project.title.clone()).into())
        })
        .collect()
}

fn scope_includes(scope: &ListScope, section: Option<&TaskSection>) -> bool {
    match scope {
        ListScope::Default => section.is_none(),
        ListScope::Section(expected) => section.is_some_and(|section| {
            section.case_insensitive_key() == expected.case_insensitive_key()
        }),
        ListScope::All => true,
    }
}

fn list_layout(scope: &ListScope) -> ListLayout {
    if matches!(scope, ListScope::All) {
        ListLayout::BySection
    } else {
        ListLayout::Flat
    }
}

fn effort_matches(task: &ListedTask, wanted: Option<EffortTier>) -> bool {
    let Some(wanted) = wanted else { return true };
    task.effort == Some(wanted)
}

fn priority_matches(task: &ListedTask, wanted: Option<PriorityTier>) -> bool {
    let Some(wanted) = wanted else { return true };
    task.priority == Some(wanted)
}

fn task_order_cmp(order: OrderSpec, a: &ListedTask, b: &ListedTask) -> std::cmp::Ordering {
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
        OrderField::Priority => directed_cmp(order.direction, a.priority.cmp(&b.priority))
            .then_with(|| id_desc_cmp(a, b)),
        OrderField::Title => {
            directed_cmp(order.direction, a.heading.as_ref().cmp(b.heading.as_ref()))
                .then_with(|| id_desc_cmp(a, b))
        }
        OrderField::Effort => {
            let effort = match (a.effort, b.effort) {
                (Some(a), Some(b)) => directed_cmp(order.direction, a.cmp(&b)),
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, None) => std::cmp::Ordering::Equal,
            };
            effort.then_with(|| id_desc_cmp(a, b))
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

fn id_desc_cmp(a: &ListedTask, b: &ListedTask) -> std::cmp::Ordering {
    b.id.number()
        .cmp(&a.id.number())
        .then_with(|| b.id.cmp(&a.id))
}

fn directed_cmp(direction: OrderDirection, ascending: std::cmp::Ordering) -> std::cmp::Ordering {
    match direction {
        OrderDirection::Asc => ascending,
        OrderDirection::Desc => ascending.reverse(),
    }
}

fn sort_by_order(tasks: &mut [ListedTask], order: OrderSpec) {
    tasks.sort_by(|a, b| task_order_cmp(order, a, b));
}

fn sort_by_group_then_order(tasks: &mut [ListedTask], order: OrderSpec) {
    let mut decorated: Vec<_> = tasks
        .iter()
        .enumerate()
        .map(|(index, task)| {
            (
                task.section.as_ref().map(TaskSection::case_insensitive_key),
                index,
            )
        })
        .collect();
    decorated.sort_by(|(section_a, index_a), (section_b, index_b)| {
        section_a
            .cmp(section_b)
            .then_with(|| task_order_cmp(order, &tasks[*index_a], &tasks[*index_b]))
    });
    let mut destinations = vec![0; tasks.len()];
    for (destination, (_, source)) in decorated.into_iter().enumerate() {
        destinations[source] = destination;
    }
    for index in 0..tasks.len() {
        while destinations[index] != index {
            let destination = destinations[index];
            tasks.swap(index, destination);
            destinations.swap(index, destination);
        }
    }
}

fn apply_cap(tasks: Vec<ListedTask>, cap: Option<usize>) -> (Vec<ListedTask>, usize) {
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
        settings::UserSettings,
        task::{
            EffortTier, PriorityTier, TaskId, TaskStatus, TaskTags,
            order::{OrderDirection, OrderField, OrderSpec},
        },
    };
    use pwf_wire::task::{
        BlockedByResolution, BlockedByStatus, ListDetail, ListLayout, ListScope, ListedTasks,
        ProjectTaskPath, RawTaskTags, StatusFilter, TaskIndexPath, TaskListLimit, TaskNotePath,
        TaskPageSize,
    };

    use super::{ListTasks, ListTasksError};
    use crate::{
        ports::{
            project_task_location::ProjectTaskLocationClient,
            task_vault::{IndexPlacement, Materialization, TaskRecord},
            user_settings::{UserSettingsLoadError, UserSettingsReader},
        },
        task::list_tasks,
        testing::{
            InMemoryStore, MIGRATOR, insert_project, project, stored_blocked_by, task_record,
            task_timestamp,
        },
    };

    #[derive(Clone, Default)]
    struct FixedSettings(UserSettings);

    impl UserSettingsReader for FixedSettings {
        fn load(&self) -> Result<UserSettings, UserSettingsLoadError> {
            Ok(self.0)
        }
    }

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

        let result = list_tasks::execute(
            &query,
            &store,
            &pool,
            &store,
            &list_tasks::ListTasksSnapshots::default(),
            &FixedSettings::default(),
        )
        .await
        .unwrap();

        assert_eq!(
            result.tasks[0]
                .details
                .as_ref()
                .unwrap()
                .blocked_by_statuses,
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
        run_with_snapshots(
            store,
            registry,
            query,
            &list_tasks::ListTasksSnapshots::default(),
        )
        .await
    }

    async fn run_with_snapshots(
        store: &InMemoryStore,
        registry: &[Project],
        query: &ListTasks,
        snapshots: &list_tasks::ListTasksSnapshots,
    ) -> Result<ListedTasks, ListTasksError> {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        MIGRATOR.run(&pool).await.unwrap();
        for project in registry {
            insert_project(
                &pool,
                project.id.as_ref(),
                project.title.as_ref(),
                project.source.as_ref().unwrap().value().as_ref(),
                project.tasks.path().as_ref(),
                project.is_paused,
            )
            .await;
        }
        list_tasks::execute(
            query,
            store,
            &pool,
            store,
            snapshots,
            &FixedSettings::default(),
        )
        .await
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
            page_size: None,
            page_token: None,
        }
    }

    #[tokio::test]
    async fn pagination_returns_stable_nonoverlapping_pages() -> Result<(), ListTasksError> {
        let snapshots = list_tasks::ListTasksSnapshots::default();
        let (store, registry) = foo_store(vec![
            record("FOO-0003"),
            record("FOO-0001"),
            record("FOO-0002"),
        ]);
        let mut query = default_query();
        query.order = Some(OrderSpec {
            field: OrderField::Id,
            direction: OrderDirection::Asc,
        });
        query.page_size = TaskPageSize::try_new(2).ok();

        let first = run_with_snapshots(&store, &registry, &query, &snapshots).await?;
        assert_eq!(listed_ids(&first), ["FOO-0001", "FOO-0002"]);
        let token = first
            .next_page_token
            .ok_or(ListTasksError::InvalidPageToken {
                reason: "first page did not continue",
            })?;

        query.page_token = Some(token);
        let second = run_with_snapshots(&store, &registry, &query, &snapshots).await?;
        assert_eq!(listed_ids(&second), ["FOO-0003"]);
        assert_eq!(second.next_page_token, None);
        Ok(())
    }

    #[tokio::test]
    async fn pagination_token_is_bound_to_the_resolved_query() {
        let (store, registry) = foo_store(vec![
            record("FOO-0001"),
            record("FOO-0002"),
            record("FOO-0003"),
        ]);
        let mut query = default_query();
        query.order = Some(OrderSpec {
            field: OrderField::Id,
            direction: OrderDirection::Asc,
        });
        query.page_size = TaskPageSize::try_new(1).ok();
        let first = run(&store, &registry, &query).await.unwrap();

        query.page_token = first.next_page_token;
        query.order = Some(OrderSpec {
            field: OrderField::Id,
            direction: OrderDirection::Desc,
        });
        let error = run(&store, &registry, &query).await.unwrap_err();

        assert!(matches!(
            error,
            ListTasksError::InvalidPageToken {
                reason: "filters or ordering changed"
            }
        ));
    }

    fn listed_ids(result: &ListedTasks) -> Vec<&str> {
        result.tasks.iter().map(|task| task.id.as_ref()).collect()
    }

    #[test]
    fn grouped_sort_preserves_every_order_and_case_insensitive_section() {
        let records = [
            record("FOO-0003"),
            sectioned("FOO-0001", "alpha"),
            sectioned("AUX-0005", "Beta"),
            sectioned("AUX-0002", "ALPHA"),
            record("AUX-0004"),
            sectioned("FOO-0006", "beta"),
        ];
        let tasks: Vec<_> = records
            .into_iter()
            .map(|record| {
                let name = if record.id.as_ref().starts_with("FOO") {
                    "foo"
                } else {
                    "aux"
                };
                crate::task::task_view::summarize(
                    record.into(),
                    ProjectName::try_new(name).unwrap(),
                )
                .unwrap()
            })
            .collect();
        for (field, direction) in [
            (OrderField::Created, OrderDirection::Asc),
            (OrderField::Created, OrderDirection::Desc),
            (OrderField::Id, OrderDirection::Asc),
            (OrderField::Id, OrderDirection::Desc),
            (OrderField::ProjectId, OrderDirection::Asc),
            (OrderField::ProjectId, OrderDirection::Desc),
        ] {
            let order = OrderSpec { field, direction };
            let mut expected = tasks.clone();
            super::sort_by_order(&mut expected, order);
            expected.sort_by_cached_key(|task| {
                task.section
                    .as_ref()
                    .map(pwf_models::task::TaskSection::case_insensitive_key)
            });
            let mut actual = tasks.clone();
            super::sort_by_group_then_order(&mut actual, order);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn cursor_decoding_rejects_invalid_versions_and_task_ids() {
        use base64::Engine as _;
        for payload in [
            r#"{"version":1,"binding":"query","after":"FOO-0001","snapshot":"id"}"#,
            r#"{"version":2,"binding":"query","after":"FOO-0001"}"#,
            r#"{"version":1,"binding":"query","after":"invalid"}"#,
        ] {
            let token =
                pwf_wire::task::TaskPageToken::try_new(super::URL_SAFE_NO_PAD.encode(payload))
                    .unwrap();
            assert!(matches!(
                super::decode_page_token(&token),
                Err(ListTasksError::InvalidPageToken { .. })
            ));
        }
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
            got.tasks[0].details.as_ref().unwrap().blocked_by_statuses,
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
            got.tasks[0].details.as_ref().unwrap().blocked_by_statuses,
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
    async fn default_scope_hides_every_sectioned_task() {
        let (store, registry) = foo_store(vec![
            record("FOO-0004"),
            sectioned("FOO-0003", "Blocked"),
            sectioned("FOO-0002", "Waiting on API"),
            sectioned("FOO-0001", "Someday"),
        ]);

        let got = run(&store, &registry, &default_query()).await.unwrap();

        assert_eq!(listed_ids(&got), ["FOO-0004"]);
    }

    #[tokio::test]
    async fn section_scope_matches_the_complete_header_case_insensitively() {
        let (store, registry) = foo_store(vec![
            sectioned("FOO-0002", "Waiting on API"),
            sectioned("FOO-0001", "Waiting"),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                scope: ListScope::Section("waiting ON api".parse().unwrap()),
                ..default_query()
            },
        )
        .await
        .unwrap();

        assert_eq!(listed_ids(&got), ["FOO-0002"]);
        assert_eq!(
            got.tasks[0].section.as_ref().map(AsRef::as_ref),
            Some("Waiting on API")
        );
    }

    #[tokio::test]
    async fn all_scope_groups_unsectioned_then_alphabetical_dynamic_sections() {
        let (store, registry) = foo_store(vec![
            sectioned("FOO-0004", "Zulu"),
            sectioned("FOO-0003", "alpha"),
            sectioned("FOO-0002", "ALPHA"),
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
            ["FOO-0001", "FOO-0003", "FOO-0002", "FOO-0004"]
        );
    }

    #[tokio::test]
    async fn section_scope_shows_only_the_requested_section() {
        let (store, registry) = foo_store(vec![
            record("FOO-0003"),
            sectioned("FOO-0002", "Blocked"),
            sectioned("FOO-0001", "Someday"),
        ]);

        let got = run(
            &store,
            &registry,
            &ListTasks {
                scope: ListScope::Section("Blocked".parse().unwrap()),
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
                section: Some("Excluded".parse().unwrap()),
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
    async fn id_desc_is_default_and_flat_across_projects() {
        let (store, registry) = store_and_registry(&[
            in_project("foo", dated_task("FOO-0001", "2026-01-01")),
            in_project("companion-project", dated_task("AUX-0001", "2026-03-01")),
        ]);

        let got = run(&store, &registry, &default_query()).await.unwrap();

        assert_eq!(listed_ids(&got), ["FOO-0001", "AUX-0001"]);
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

    #[tokio::test]
    async fn new_sorts_use_tier_order_title_text_and_descending_id_ties() {
        let records = [
            ("FOO-0001", "Beta", Some("high"), Some("high")),
            ("FOO-0002", "alpha", Some("highest"), Some("low")),
            ("FOO-0003", "Alpha", None, None),
            ("FOO-0004", "zeta", Some("low"), Some("highest")),
            ("FOO-0005", "beta", Some("high"), Some("medium")),
            ("FOO-0006", "omega", Some("medium"), None),
        ]
        .into_iter()
        .map(|(id, title, priority, effort)| TaskRecord {
            title: title.to_string(),
            priority: priority.map(str::to_string),
            effort: effort.map(str::to_string),
            ..record(id)
        })
        .collect();
        let (store, registry) = foo_store(records);
        for (order, numbers) in [
            ("priority", [2, 5, 1, 6, 3, 4]),
            ("priority:asc", [4, 6, 3, 5, 1, 2]),
            ("effort", [2, 5, 1, 4, 6, 3]),
            ("effort:desc", [4, 1, 5, 2, 6, 3]),
            ("title", [3, 2, 5, 1, 6, 4]),
            ("title:desc", [4, 6, 5, 1, 3, 2]),
        ] {
            let result = run(
                &store,
                &registry,
                &ListTasks {
                    order: Some(order.parse().unwrap()),
                    ..default_query()
                },
            )
            .await
            .unwrap();
            let expected: Vec<_> = numbers.map(|number| format!("FOO-{number:04}")).into();
            assert_eq!(listed_ids(&result), expected, "{order}");
        }
        let result = run(
            &store,
            &registry,
            &ListTasks {
                priority: Some(PriorityTier::Medium),
                ..default_query()
            },
        )
        .await
        .unwrap();
        assert_eq!(listed_ids(&result), ["FOO-0006", "FOO-0003"]);
        assert!(
            result
                .tasks
                .iter()
                .all(|task| task.priority == Some(PriorityTier::Medium))
        );
    }

    #[tokio::test]
    async fn all_keeps_sections_before_priority_and_cap() {
        let (store, registry) = foo_store(vec![
            TaskRecord {
                priority: Some("low".to_string()),
                ..sectioned("FOO-0001", "alpha")
            },
            TaskRecord {
                priority: Some("highest".to_string()),
                ..sectioned("FOO-0002", "zeta")
            },
            TaskRecord {
                priority: Some("high".to_string()),
                ..sectioned("FOO-0003", "alpha")
            },
        ]);
        let mut query = ListTasks {
            scope: ListScope::All,
            order: Some("priority".parse().unwrap()),
            ..default_query()
        };
        assert_eq!(
            listed_ids(&run(&store, &registry, &query).await.unwrap()),
            ["FOO-0003", "FOO-0001", "FOO-0002"]
        );
        query.number = TaskListLimit::try_new(1).ok();
        let capped = run(&store, &registry, &query).await.unwrap();
        assert_eq!(listed_ids(&capped), ["FOO-0003"]);
        assert_eq!(capped.hidden, 2);
    }
}
