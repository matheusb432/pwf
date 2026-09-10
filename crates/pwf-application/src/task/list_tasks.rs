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
    project::ProjectStatusFilter,
    task::{
        ListDetail, ListLayout, ListScope, ListTasks, ListedTask, ListedTasks, StatusFilter,
        TaskPageSize, TaskPageToken,
    },
};
use serde::{Deserialize, Serialize};
pub use snapshots::ListTasksSnapshots;

use super::{blocked_by, task_projection};
use crate::{
    ports::{
        project_task_location::ProjectTaskLocationClient,
        task_vault::TaskVault,
        user_settings::{UserSettingsLoadError, UserSettingsReader},
    },
    project::{get_active_project, get_projects, list_projects},
};

/// Retains invalid requested or persisted tag text for list diagnostics.
#[derive(Debug, thiserror::Error)]
#[error("{source}")]
pub struct TagParseError {
    raw: String,
    #[source]
    source: pwf_models::task::ParseTaskTagsError,
}

impl TagParseError {
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }
}

impl From<pwf_models::task::ParseTaskTagsError> for TagParseError {
    fn from(error: pwf_models::task::ParseTaskTagsError) -> Self {
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
    InvalidTaskProjection(anyhow::Error),
    #[error(transparent)]
    GetProject(#[from] crate::project::get_project::GetProjectError),
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
    let selected = match query.project_id.as_ref() {
        Some(id) => Some(get_active_project::execute(id, pool).await?),
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
    let project_task_path = query
        .project
        .as_ref()
        .map(|project| task_locations.project_task_path(project))
        .transpose()
        .map_err(|source| ListTasksError::ReadProjectTaskPath(anyhow::Error::new(source)))?;
    let mut result =
        collect_page(&query, cursor.as_ref(), &binding, store, pool, snapshots).await?;

    if query.detail.includes_relationship_statuses() {
        let project_ids = result
            .page
            .items
            .iter()
            .filter_map(|task| task.details.as_ref()?.blocked_by.as_ref())
            .flat_map(|blockers| blockers.iter().map(TaskId::project_id))
            .filter(|id| {
                query
                    .project
                    .as_ref()
                    .is_none_or(|project| &project.id != *id)
            })
            .cloned()
            .collect();
        let relationship_projects = get_projects::execute(&project_ids, pool)
            .await
            .map_err(|error| ListTasksError::QueryProject(anyhow::Error::new(error)))?;
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
            pwf_models::task::TaskTags::parse_frontmatter(raw.as_ref()).map_err(|source| {
                ListTasksError::InvalidTags {
                    id: task.id.clone(),
                    source: source.into(),
                }
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
                task_projection::summarize(record, project.title.clone()).map_err(|error| {
                    ListTasksError::InvalidTaskProjection(anyhow::Error::new(error))
                })
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
            task_projection::detailed(
                &record,
                project.title.clone(),
                project
                    .source
                    .as_ref()
                    .map(pwf_models::project::ProjectSource::value),
            )
            .map_err(|error| ListTasksError::InvalidTaskProjection(anyhow::Error::new(error)))
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
    use pwf_models::project::ProjectName;
    use pwf_wire::task::TaskRecord;

    use super::*;
    use crate::testing::task_record;
    fn sectioned(id: &str, section: &str) -> TaskRecord {
        TaskRecord {
            section: Some(section.parse().unwrap()),
            ..task_record(id)
        }
    }
    #[test]
    fn grouped_sort_preserves_every_order_and_case_insensitive_section() {
        let records = [
            task_record("FOO-0003"),
            sectioned("FOO-0001", "alpha"),
            sectioned("AUX-0005", "Beta"),
            sectioned("AUX-0002", "ALPHA"),
            task_record("AUX-0004"),
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
                crate::task::task_projection::summarize(
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
}
