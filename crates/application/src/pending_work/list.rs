use pwf_domain::pending_work::{
    ListResult, ListScope, OrderDirection, OrderField, OrderSpec, ParseTagsError,
    PendingWorkItemView, ProjectName, ProjectRegistry, Tags, WorkItemStatus, WorkItemStatusFilter,
};

use crate::{
    AppDbStore, PendingWorkItem,
    pending_work::enrich::{enrich, is_open_item},
};

const DEFAULT_LIST_CAP: usize = 10;

#[derive(Debug, Clone)]
pub struct GetPendingWork {
    pub only_project: Option<String>,
    pub scope: ListScope,
    pub number: Option<usize>,
    pub effort: Option<u8>,
    pub tags: Option<Tags>,
    pub order: OrderSpec,
    /// Selects one persisted lifecycle status or every lifecycle status.
    pub status_filter: WorkItemStatusFilter,
}

#[derive(Debug, thiserror::Error)]
pub enum GetPendingWorkError {
    #[error("pending-work read failed: {0}")]
    ReadStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("item {id} has invalid tags frontmatter: {source}")]
    InvalidTags {
        id: String,
        #[source]
        source: ParseTagsError,
    },
    #[error("Invalid project identity: {value:?}.")]
    InvalidProject { value: String },
}

#[cqrsy::query]
pub fn execute(
    query: &GetPendingWork,
    store: &impl AppDbStore<PendingWorkItem>,
    projects: &ProjectRegistry,
) -> Result<ListResult, GetPendingWorkError> {
    let mut items = collect_list_items(query, store, projects)?;

    items.retain(|item| scope_includes(query.scope, item.section.as_deref()));
    items.retain(|item| effort_matches(item, query.effort));

    if let Some(requested) = query.tags.as_ref() {
        let mut matched = Vec::with_capacity(items.len());
        for item in items {
            let Some(raw) = item.tags.as_deref() else {
                continue;
            };
            let stored = Tags::parse_frontmatter(raw).map_err(|source| {
                GetPendingWorkError::InvalidTags {
                    id: item.id.clone(),
                    source,
                }
            })?;
            if stored.contains_all(requested) {
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

    let (items, hidden) = apply_cap(items, query.number.unwrap_or(DEFAULT_LIST_CAP));

    Ok(ListResult { items, hidden })
}

/// Reads and enriches listable lifecycle records from one project or every project in name order.
fn collect_list_items(
    query: &GetPendingWork,
    store: &impl AppDbStore<PendingWorkItem>,
    projects: &ProjectRegistry,
) -> Result<Vec<PendingWorkItemView>, GetPendingWorkError> {
    let scan: Vec<(ProjectName, Option<String>)> = match query.only_project.as_deref() {
        Some(name) => {
            let project =
                ProjectName::try_new(name).map_err(|_| GetPendingWorkError::InvalidProject {
                    value: name.to_string(),
                })?;
            let repo = projects.repo_for(&project).map(str::to_string);
            vec![(project, repo)]
        }
        None => projects
            .projects()
            .map(|(name, repo)| (name.clone(), repo.map(str::to_string)))
            .collect(),
    };

    let mut items = Vec::new();
    for (project, repo) in &scan {
        let records = store
            .list(project)
            .map_err(|error| GetPendingWorkError::ReadStore(Box::new(error)))?;
        for record in records {
            let listable = is_open_item(&record) || record.status != WorkItemStatus::Active;
            if !listable || !query.status_filter.includes(record.status) {
                continue;
            }
            items.push(
                enrich(&record, repo.as_deref())
                    .into_pending_work_item_view(project.as_ref().to_string()),
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

fn effort_matches(item: &PendingWorkItemView, wanted: Option<u8>) -> bool {
    let Some(wanted) = wanted else { return true };
    item.effort.as_deref().and_then(parse_effort_tier) == Some(wanted)
}

fn parse_effort_tier(raw: &str) -> Option<u8> {
    raw.trim()
        .parse::<u8>()
        .ok()
        .filter(|tier| (1..=4).contains(tier))
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

fn apply_cap(items: Vec<PendingWorkItemView>, cap: usize) -> (Vec<PendingWorkItemView>, usize) {
    if cap == 0 || items.len() <= cap {
        return (items, 0);
    }

    let hidden = items.len() - cap;
    let mut kept = items;
    kept.truncate(cap);
    (kept, hidden)
}

#[cfg(test)]
mod tests {
    use pwf_domain::pending_work::{
        ListResult, ListScope, OrderDirection, OrderField, OrderSpec, ProjectName, ProjectRegistry,
        Tags, Timestamp, WorkItemId, WorkItemStatus, WorkItemStatusFilter,
    };

    use super::{GetPendingWork, GetPendingWorkError, execute};
    use crate::{
        IndexPlacement, Materialization, PendingWorkItem, RecordId, testing::InMemoryStore,
    };

    type Staged = (&'static str, PendingWorkItem);

    fn record(id: &str) -> PendingWorkItem {
        PendingWorkItem {
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

    fn in_project(project: &'static str, item: PendingWorkItem) -> Staged {
        (project, item)
    }

    fn store_and_registry(items: &[Staged]) -> (InMemoryStore, ProjectRegistry) {
        let mut store = InMemoryStore::default();
        let mut projects: Vec<&'static str> = items.iter().map(|(project, _)| *project).collect();
        projects.sort_unstable();
        projects.dedup();
        for project in &projects {
            let staged: Vec<PendingWorkItem> = items
                .iter()
                .filter(|(candidate, _)| candidate == project)
                .map(|(_, item)| item.clone())
                .collect();
            store = store.with_project(project, staged);
        }
        let registry = ProjectRegistry::new(projects.iter().map(|project| {
            (
                ProjectName::try_new(*project).unwrap(),
                Some(format!("/repo/{project}")),
                None,
            )
        }));
        (store, registry)
    }

    fn pwf_store(items: Vec<PendingWorkItem>) -> (InMemoryStore, ProjectRegistry) {
        let staged: Vec<Staged> = items.into_iter().map(|item| ("pwf", item)).collect();
        store_and_registry(&staged)
    }

    fn run(
        store: &InMemoryStore,
        registry: &ProjectRegistry,
        query: &GetPendingWork,
    ) -> Result<ListResult, GetPendingWorkError> {
        execute(query, store, registry)
    }

    fn sectioned(id: &str, section: &str) -> PendingWorkItem {
        PendingWorkItem {
            section: Some(section.to_string()),
            ..record(id)
        }
    }

    fn effort_item(id: &str, effort: &str) -> PendingWorkItem {
        PendingWorkItem {
            effort: Some(effort.to_string()),
            ..record(id)
        }
    }

    fn tagged_item(id: &str, tags: &str) -> PendingWorkItem {
        PendingWorkItem {
            tags: Some(tags.to_string()),
            ..record(id)
        }
    }

    fn dated_item(id: &str, created: &str) -> PendingWorkItem {
        PendingWorkItem {
            created: Some(Timestamp::new(created)),
            ..record(id)
        }
    }

    fn default_query() -> GetPendingWork {
        GetPendingWork {
            only_project: None,
            scope: ListScope::Default,
            number: None,
            effort: None,
            tags: None,
            order: OrderSpec::default(),
            status_filter: WorkItemStatusFilter::default(),
        }
    }

    fn listed_ids(result: &ListResult) -> Vec<&str> {
        result.items.iter().map(|item| item.id.as_str()).collect()
    }

    #[test]
    fn list_filters_active_only() {
        let done = PendingWorkItem {
            status: WorkItemStatus::Done,
            ..record("PWF-0002")
        };
        let cancelled = PendingWorkItem {
            status: WorkItemStatus::Cancelled,
            ..record("PWF-0003")
        };
        let (store, registry) = pwf_store(vec![record("PWF-0001"), done, cancelled]);

        let got = run(&store, &registry, &default_query()).unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0001"]);
    }

    #[test]
    fn list_status_filter_selects_exact_statuses_and_all() {
        let done = PendingWorkItem {
            status: WorkItemStatus::Done,
            placement: None,
            ..record("PWF-0002")
        };
        let cancelled = PendingWorkItem {
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
                    status_filter: WorkItemStatusFilter::Exact(status),
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
                status_filter: WorkItemStatusFilter::All,
                ..default_query()
            },
        )
        .unwrap();
        assert_eq!(listed_ids(&all), ["PWF-0003", "PWF-0002", "PWF-0001"]);
    }

    #[test]
    fn active_orphan_is_hidden_from_active_and_all_lists() {
        let orphan = PendingWorkItem {
            placement: None,
            ..record("PWF-0002")
        };
        let (store, registry) = pwf_store(vec![record("PWF-0001"), orphan]);

        for status_filter in [
            WorkItemStatusFilter::Exact(WorkItemStatus::Active),
            WorkItemStatusFilter::All,
        ] {
            let got = run(
                &store,
                &registry,
                &GetPendingWork {
                    status_filter,
                    ..default_query()
                },
            )
            .unwrap();
            assert_eq!(listed_ids(&got), ["PWF-0001"]);
        }
    }

    #[test]
    fn status_filter_applies_before_cap_and_hidden_count() {
        let active = PendingWorkItem {
            created: Some(Timestamp::new("2026-07-09")),
            ..record("PWF-0009")
        };
        let done_newer = PendingWorkItem {
            status: WorkItemStatus::Done,
            placement: None,
            created: Some(Timestamp::new("2026-07-08")),
            ..record("PWF-0002")
        };
        let done_older = PendingWorkItem {
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
                status_filter: WorkItemStatusFilter::Exact(WorkItemStatus::Done),
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
                scope: ListScope::FutureOnly,
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0001"]);
    }

    #[test]
    fn all_scope_groups_by_section_rank() {
        let (store, registry) = pwf_store(vec![
            sectioned("GLP-0004", "Future"),
            sectioned("GLP-0003", "Human"),
            sectioned("GLP-0002", "Low-prio"),
            record("GLP-0001"),
        ]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                scope: ListScope::All,
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(
            listed_ids(&got),
            ["GLP-0001", "GLP-0002", "GLP-0003", "GLP-0004"]
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
                scope: ListScope::HumanOnly,
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0002"]);
    }

    #[test]
    fn effort_filter_matches_exact_tier_only() {
        let (store, registry) = pwf_store(vec![
            effort_item("PWF-0003", "3"),
            effort_item("PWF-0002", "2"),
            record("PWF-0001"),
        ]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                effort: Some(3),
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0003"]);
    }

    #[test]
    fn stored_effort_trims_and_bounds_the_tier() {
        let (store, registry) = pwf_store(vec![
            effort_item("PWF-0003", " 3 "),
            effort_item("PWF-0002", "0"),
            effort_item("PWF-0001", "5"),
        ]);

        let matched = run(
            &store,
            &registry,
            &GetPendingWork {
                effort: Some(3),
                ..default_query()
            },
        )
        .unwrap();
        assert_eq!(listed_ids(&matched), ["PWF-0003"]);

        for tier in [0, 5] {
            let empty = run(
                &store,
                &registry,
                &GetPendingWork {
                    effort: Some(tier),
                    ..default_query()
                },
            )
            .unwrap();
            assert!(empty.items.is_empty(), "tier {tier} should not match");
        }
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
                tags: Some(Tags::parse_values(&["SQLite,godot".to_string()]).unwrap()),
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
                tags: Some(Tags::parse_values(&["sqlite".to_string()]).unwrap()),
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
            PendingWorkItem {
                section: Some("Human".to_string()),
                ..tagged_item("PWF-0003", "corrupt")
            },
            PendingWorkItem {
                effort: Some("2".to_string()),
                ..tagged_item("PWF-0002", "also corrupt")
            },
            PendingWorkItem {
                effort: Some("3".to_string()),
                ..tagged_item("PWF-0001", "[sqlite]")
            },
        ]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                effort: Some(3),
                tags: Some(Tags::parse_values(&["sqlite".to_string()]).unwrap()),
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
                tags: Some(Tags::parse_values(&["sqlite".to_string()]).unwrap()),
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
            dated_item("GLP-0001", "2026-01-01"),
            dated_item("GLP-0002", "2026-03-01"),
            dated_item("GLP-0003", "2026-02-01"),
        ]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                order: OrderSpec {
                    field: OrderField::Created,
                    direction: OrderDirection::Asc,
                },
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["GLP-0001", "GLP-0003", "GLP-0002"]);
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
                order: OrderSpec {
                    field: OrderField::Id,
                    direction: OrderDirection::Desc,
                },
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0099", "CFG-0001"]);
    }

    #[test]
    fn project_id_order_groups_by_project_then_newest_id() {
        let (store, registry) = store_and_registry(&[
            in_project("pwf", record("PWF-9999")),
            in_project("config-handler", record("CFG-0001")),
            in_project("config-handler", record("CFG-0002")),
        ]);

        let got = run(
            &store,
            &registry,
            &GetPendingWork {
                order: OrderSpec {
                    field: OrderField::ProjectId,
                    direction: OrderDirection::Asc,
                },
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["CFG-0002", "CFG-0001", "PWF-9999"]);
    }

    #[test]
    fn number_zero_keeps_all_items_and_default_cap_hides_after_ten() {
        let (store, registry) =
            pwf_store((1..=12).map(|n| record(&format!("GLP-{n:04}"))).collect());

        let all = run(
            &store,
            &registry,
            &GetPendingWork {
                number: Some(0),
                ..default_query()
            },
        )
        .unwrap();
        assert_eq!(all.items.len(), 12);
        assert_eq!(all.hidden, 0);

        let capped = run(&store, &registry, &default_query()).unwrap();
        assert_eq!(capped.items.len(), 10);
        assert_eq!(capped.hidden, 2);
    }

    #[test]
    fn inline_legacy_records_list_with_project_scoped_ids() {
        let inline = PendingWorkItem {
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
                only_project: Some("pwf".to_string()),
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0001"]);
    }
}
