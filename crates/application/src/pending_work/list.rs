use pwf_domain::pending_work::{
    ListResult, ListScope, OpenItem, OrderDirection, OrderField, OrderSpec, ParseTagsError,
    ProjectName, Tags,
};

use crate::ports::PendingWorkReadStore;

const DEFAULT_LIST_CAP: usize = 10;

#[derive(Debug, Clone)]
pub struct GetPendingWork {
    pub only_project: Option<String>,
    pub scope: ListScope,
    pub number: Option<usize>,
    pub effort: Option<u8>,
    pub tags: Option<Tags>,
    pub order: OrderSpec,
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

#[cqrsy::handler(query)]
pub fn handle(
    store: &impl PendingWorkReadStore,
    query: GetPendingWork,
) -> Result<ListResult, GetPendingWorkError> {
    let mut items = match query.only_project.as_deref() {
        Some(project) => {
            let project =
                ProjectName::try_new(project).map_err(|_| GetPendingWorkError::InvalidProject {
                    value: project.to_string(),
                })?;
            store.open_items_for_project(&project)
        }
        None => store.all_open_items(),
    }
    .map_err(|error| GetPendingWorkError::ReadStore(Box::new(error)))?;

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

fn effort_matches(item: &OpenItem, wanted: Option<u8>) -> bool {
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

fn created_key(item: &OpenItem) -> &str {
    item.created.as_deref().unwrap_or("")
}

fn item_order_cmp(order: OrderSpec, a: &OpenItem, b: &OpenItem) -> std::cmp::Ordering {
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

fn sort_by_order(items: &mut [OpenItem], order: OrderSpec) {
    items.sort_by(|a, b| item_order_cmp(order, a, b));
}

fn sort_by_group_then_order(items: &mut [OpenItem], order: OrderSpec) {
    items.sort_by(|a, b| {
        section_group_rank(a.section.as_deref())
            .cmp(&section_group_rank(b.section.as_deref()))
            .then_with(|| item_order_cmp(order, a, b))
    });
}

fn apply_cap(items: Vec<OpenItem>, cap: usize) -> (Vec<OpenItem>, usize) {
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
    use cqrsy::Sender;
    use pwf_domain::pending_work::{
        ListResult, ListScope, OpenItem, OrderDirection, OrderField, OrderSpec, Tags,
    };

    use super::{GetPendingWork, GetPendingWorkError, GetPendingWorkHandler};
    use crate::{ports::PendingWorkReadStore, testing::InMemoryPendingWorkReadStore};

    /// Sync dispatch shim: drive a fused `GetPendingWorkHandler` through its
    /// blanket `Sender` on the tested path, keeping each case's assertions
    /// focused on the query result.
    fn send_now<S: PendingWorkReadStore>(
        handler: &GetPendingWorkHandler<S>,
        query: GetPendingWork,
    ) -> Result<ListResult, GetPendingWorkError> {
        handler.send_now(query)
    }

    fn item(id: &str) -> OpenItem {
        OpenItem {
            id: id.to_string(),
            project: "pwf".to_string(),
            session: "pwf".to_string(),
            prompt: String::new(),
            repo: Some("/repo/pwf".to_string()),
            note: String::new(),
            item_file: None,
            line: 1,
            format: "file".to_string(),
            launchable: true,
            needs_prompt: false,
            issues: vec![],
            section: None,
            prereq: None,
            effort: None,
            tags: None,
            created: Some("2026-07-07".to_string()),
        }
    }

    fn human_item(id: &str) -> OpenItem {
        OpenItem {
            section: Some("Human".to_string()),
            ..item(id)
        }
    }

    fn future_item(id: &str) -> OpenItem {
        OpenItem {
            section: Some("Future".to_string()),
            ..item(id)
        }
    }

    fn low_prio_item(id: &str) -> OpenItem {
        OpenItem {
            section: Some("Low-prio".to_string()),
            ..item(id)
        }
    }

    fn effort_item(id: &str, effort: &str) -> OpenItem {
        OpenItem {
            effort: Some(effort.to_string()),
            ..item(id)
        }
    }

    fn tagged_item(id: &str, tags: &str) -> OpenItem {
        OpenItem {
            tags: Some(tags.to_string()),
            ..item(id)
        }
    }

    fn project_item(project: &str, id: &str) -> OpenItem {
        OpenItem {
            id: id.to_string(),
            project: project.to_string(),
            repo: Some(format!("/repo/{project}")),
            session: project.to_string(),
            ..item(id)
        }
    }

    fn dated_item(id: &str, created: &str) -> OpenItem {
        OpenItem {
            created: Some(created.to_string()),
            ..item(id)
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
        }
    }

    fn listed_ids(result: &pwf_domain::pending_work::ListResult) -> Vec<&str> {
        result.items.iter().map(|item| item.id.as_str()).collect()
    }

    #[test]
    fn default_scope_hides_human_and_future_sections() {
        let store = InMemoryPendingWorkReadStore::with_items(vec![
            item("PWF-0003"),
            human_item("PWF-0002"),
            future_item("PWF-0001"),
        ]);

        let got = send_now(&GetPendingWorkHandler { store }, default_query()).unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0003"]);
    }

    #[test]
    fn default_scope_hides_low_prio_too() {
        let store = InMemoryPendingWorkReadStore::with_items(vec![
            item("PWF-0002"),
            low_prio_item("PWF-0001"),
        ]);

        let got = send_now(&GetPendingWorkHandler { store }, default_query()).unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0002"]);
    }

    #[test]
    fn all_scope_groups_by_section_rank() {
        let store = InMemoryPendingWorkReadStore::with_items(vec![
            future_item("GLP-0004"),
            human_item("GLP-0003"),
            low_prio_item("GLP-0002"),
            item("GLP-0001"),
        ]);

        let got = send_now(
            &GetPendingWorkHandler { store },
            GetPendingWork {
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
        let store = InMemoryPendingWorkReadStore::with_items(vec![
            item("PWF-0003"),
            human_item("PWF-0002"),
            future_item("PWF-0001"),
        ]);

        let got = send_now(
            &GetPendingWorkHandler { store },
            GetPendingWork {
                scope: ListScope::HumanOnly,
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0002"]);
    }

    #[test]
    fn future_scope_shows_only_future_items() {
        let store = InMemoryPendingWorkReadStore::with_items(vec![
            item("PWF-0003"),
            human_item("PWF-0002"),
            future_item("PWF-0001"),
        ]);

        let got = send_now(
            &GetPendingWorkHandler { store },
            GetPendingWork {
                scope: ListScope::FutureOnly,
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0001"]);
    }

    #[test]
    fn effort_filter_matches_exact_tier_only() {
        let store = InMemoryPendingWorkReadStore::with_items(vec![
            effort_item("PWF-0003", "3"),
            effort_item("PWF-0002", "2"),
            item("PWF-0001"),
        ]);

        let got = send_now(
            &GetPendingWorkHandler { store },
            GetPendingWork {
                effort: Some(3),
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0003"]);
    }

    #[test]
    fn stored_effort_trims_surrounding_whitespace() {
        let store = InMemoryPendingWorkReadStore::with_items(vec![effort_item("PWF-0001", " 3 ")]);

        let got = send_now(
            &GetPendingWorkHandler { store },
            GetPendingWork {
                effort: Some(3),
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0001"]);
    }

    #[test]
    fn stored_effort_zero_does_not_match_filter_zero() {
        let store = InMemoryPendingWorkReadStore::with_items(vec![effort_item("PWF-0001", "0")]);

        let got = send_now(
            &GetPendingWorkHandler { store },
            GetPendingWork {
                effort: Some(0),
                ..default_query()
            },
        )
        .unwrap();

        assert!(got.items.is_empty());
    }

    #[test]
    fn stored_effort_five_does_not_match_filter_five() {
        let store = InMemoryPendingWorkReadStore::with_items(vec![effort_item("PWF-0001", "5")]);

        let got = send_now(
            &GetPendingWorkHandler { store },
            GetPendingWork {
                effort: Some(5),
                ..default_query()
            },
        )
        .unwrap();

        assert!(got.items.is_empty());
    }

    #[test]
    fn tag_filter_requires_every_requested_tag() {
        let store = InMemoryPendingWorkReadStore::with_items(vec![
            tagged_item("PWF-0004", "[sqlite_tools, godot]"),
            tagged_item("PWF-0003", "[sqlite, godot]"),
            tagged_item("PWF-0002", "[sqlite]"),
            item("PWF-0001"),
        ]);

        let got = send_now(
            &GetPendingWorkHandler { store },
            GetPendingWork {
                tags: Some(Tags::parse_values(&["SQLite,godot".to_string()]).unwrap()),
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(listed_ids(&got), ["PWF-0003"]);
    }

    #[test]
    fn corrupt_tags_fail_only_when_a_tag_filter_is_requested() {
        let store = InMemoryPendingWorkReadStore::with_items(vec![tagged_item(
            "PWF-0001",
            "sqlite, godot",
        )]);
        assert!(
            send_now(
                &GetPendingWorkHandler {
                    store: store.clone()
                },
                default_query()
            )
            .is_ok()
        );

        let error = send_now(
            &GetPendingWorkHandler { store },
            GetPendingWork {
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
        let store = InMemoryPendingWorkReadStore::with_items(vec![
            OpenItem {
                section: Some("Human".to_string()),
                ..tagged_item("PWF-0003", "corrupt")
            },
            OpenItem {
                effort: Some("2".to_string()),
                ..tagged_item("PWF-0002", "also corrupt")
            },
            OpenItem {
                effort: Some("3".to_string()),
                ..tagged_item("PWF-0001", "[sqlite]")
            },
        ]);

        let got = send_now(
            &GetPendingWorkHandler { store },
            GetPendingWork {
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
        let store = InMemoryPendingWorkReadStore::with_items(vec![
            item("PWF-9999"),
            tagged_item("PWF-0002", "[sqlite]"),
            tagged_item("PWF-0001", "[sqlite]"),
        ]);

        let got = send_now(
            &GetPendingWorkHandler { store },
            GetPendingWork {
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
        let store = InMemoryPendingWorkReadStore::with_items(vec![
            OpenItem {
                project: "pwf".to_string(),
                ..dated_item("PWF-0001", "2026-01-01")
            },
            OpenItem {
                project: "config-handler".to_string(),
                ..dated_item("CFG-0001", "2026-03-01")
            },
        ]);

        let got = send_now(&GetPendingWorkHandler { store }, default_query()).unwrap();

        assert_eq!(listed_ids(&got), ["CFG-0001", "PWF-0001"]);
    }

    #[test]
    fn created_asc_orders_oldest_first() {
        let store = InMemoryPendingWorkReadStore::with_items(vec![
            dated_item("GLP-0001", "2026-01-01"),
            dated_item("GLP-0002", "2026-03-01"),
            dated_item("GLP-0003", "2026-02-01"),
        ]);

        let got = send_now(
            &GetPendingWorkHandler { store },
            GetPendingWork {
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
        let store = InMemoryPendingWorkReadStore::with_items(vec![
            project_item("config-handler", "CFG-0001"),
            project_item("pwf", "PWF-0099"),
        ]);

        let got = send_now(
            &GetPendingWorkHandler { store },
            GetPendingWork {
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
        let store = InMemoryPendingWorkReadStore::with_items(vec![
            project_item("pwf", "PWF-9999"),
            project_item("config-handler", "CFG-0001"),
            project_item("config-handler", "CFG-0002"),
        ]);

        let got = send_now(
            &GetPendingWorkHandler { store },
            GetPendingWork {
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
    fn number_zero_keeps_all_items() {
        let store = InMemoryPendingWorkReadStore::with_items(
            (1..=12).map(|n| item(&format!("GLP-{n:04}"))).collect(),
        );

        let got = send_now(
            &GetPendingWorkHandler { store },
            GetPendingWork {
                number: Some(0),
                ..default_query()
            },
        )
        .unwrap();

        assert_eq!(got.items.len(), 12);
        assert_eq!(got.hidden, 0);
    }

    #[test]
    fn default_cap_hides_items_after_ten() {
        let store = InMemoryPendingWorkReadStore::with_items(
            (1..=12).map(|n| item(&format!("GLP-{n:04}"))).collect(),
        );

        let got = send_now(&GetPendingWorkHandler { store }, default_query()).unwrap();

        assert_eq!(got.items.len(), 10);
        assert_eq!(got.hidden, 2);
    }
}
