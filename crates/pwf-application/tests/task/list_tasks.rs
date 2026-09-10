use std::{convert::Infallible, num::NonZeroUsize};

use pwf_application::{
    ports::{
        project_task_location::ProjectTaskLocationClient,
        user_settings::{UserSettingsLoadError, UserSettingsReader},
    },
    task::{list_tasks, list_tasks::ListTasksError},
};
use pwf_models::{
    project::{Project, ProjectName},
    settings::UserSettings,
    task::{
        EffortTier, PriorityTier, TaskId, TaskStatus, TaskTags,
        order::{OrderDirection, OrderField, OrderSpec},
    },
};
use pwf_wire::task::{
    BlockedByResolution, BlockedByStatus, IndexPlacement, ListDetail, ListLayout, ListScope,
    ListTasks, ListedTasks, Materialization, ProjectTaskPath, RawTaskTags, StatusFilter,
    TaskIndexPath, TaskListLimit, TaskNotePath, TaskPageSize, TaskRecord,
};

use crate::support::{
    InMemoryStore, MIGRATOR, insert_project, project, stored_blocked_by, task_record,
    task_timestamp,
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

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
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
        project_id: Some("foo".parse().unwrap()),
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
        project_id: None,
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
            project_id: Some("foo".parse().unwrap()),
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
            project_id: Some("foo".parse().unwrap()),
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

    assert!(matches!(
        error,
        ListTasksError::InvalidTaskProjection { .. }
    ));
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
    let (store, registry) = foo_store((1..=12).map(|n| record(&format!("FOO-{n:04}"))).collect());

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
            project_id: Some("foo".parse().unwrap()),
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

#[tokio::test]
async fn statuses_expose_store_read_failures_as_unavailable() {
    use crate::support::InMemoryStoreFailure;
    let task = TaskRecord {
        blocked_by: stored_blocked_by(&["FOO-0001"]),
        ..record("FOO-0002")
    };
    let store = InMemoryStore::default()
        .with_project("foo", vec![task])
        .with_failure(InMemoryStoreFailure::ReadTask);
    let result = run(
        &store,
        &[project("FOO", "foo")],
        &ListTasks {
            detail: ListDetail::Detailed,
            ..default_query()
        },
    )
    .await
    .unwrap();
    let statuses = &result.tasks[0]
        .details
        .as_ref()
        .unwrap()
        .blocked_by_statuses;
    assert!(matches!(statuses.as_slice(), [BlockedByStatus {
        id, resolution: BlockedByResolution::Unavailable { reason }, ..
    }] if id.as_ref() == "FOO-0001" && reason == "injected in-memory store failure: task-read"));
}
