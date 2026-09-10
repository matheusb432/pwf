use pwf_application::task::get_task_dag;
use pwf_models::task::{BlockedBy, TaskId, TaskStatus};
use pwf_wire::task::{
    GetTaskDag, StatusFilter, StoredBlockedBy, TaskDagDepth, TaskDagEdge, TaskDagMode, TaskDagNode,
};

use crate::support::{
    InMemoryStore, InMemoryStoreFailure, MIGRATOR, insert_project, stored_blocked_by, task_record,
};

#[sqlx::test(migrator = "MIGRATOR")]
async fn blocked_by_mode_traverses_cross_project_ancestors_in_stored_order(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    insert_project(&pool, "AUX", "aux", "/work/aux", "/tasks/aux", true).await;

    let root = pwf_wire::task::TaskRecord {
        title: "ship graph view".to_string(),
        blocked_by: stored_blocked_by(&["FOO-0002", "AUX-0001"]),
        ..task_record("FOO-0003")
    };
    let direct = pwf_wire::task::TaskRecord {
        title: "prepare graph data".to_string(),
        blocked_by: stored_blocked_by(&["FOO-0001"]),
        ..task_record("FOO-0002")
    };
    let ancestor = pwf_wire::task::TaskRecord {
        title: "adopt blocked by".to_string(),
        ..task_record("FOO-0001")
    };
    let paused_project_ancestor = pwf_wire::task::TaskRecord {
        title: "supply shared contract".to_string(),
        ..task_record("AUX-0001")
    };
    let store = InMemoryStore::default()
        .with_project("foo", vec![root, direct, ancestor])
        .with_project("aux", vec![paused_project_ancestor]);

    let graph = get_task_dag::execute(
        &GetTaskDag {
            id: TaskId::try_new("FOO-0003").unwrap(),
            depth: None,
            status: StatusFilter::All,
            mode: TaskDagMode::BlockedBy,
        },
        &store,
        &pool,
    )
    .await
    .unwrap();

    assert_eq!(graph.root_id().as_ref(), "FOO-0003");
    assert_eq!(
        graph
            .nodes()
            .iter()
            .map(|node| match node {
                TaskDagNode::Task { id, .. }
                | TaskDagNode::Missing { id }
                | TaskDagNode::Unavailable { id } => id.as_ref(),
                TaskDagNode::DepthLimit => "depth-limit",
            })
            .collect::<Vec<_>>(),
        ["FOO-0003", "FOO-0002", "FOO-0001", "AUX-0001"]
    );
    assert_eq!(
        graph.edges(),
        [
            TaskDagEdge {
                blocker_node_index: 1,
                dependent_node_index: 0,
            },
            TaskDagEdge {
                blocker_node_index: 2,
                dependent_node_index: 1,
            },
            TaskDagEdge {
                blocker_node_index: 3,
                dependent_node_index: 0,
            },
        ]
    );
    pool.close().await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn status_filter_keeps_the_root_and_stops_at_hidden_dependents(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    let root = pwf_wire::task::TaskRecord {
        title: "completed foundation".to_string(),
        status: TaskStatus::Done,
        ..task_record("FOO-0001")
    };
    let hidden = pwf_wire::task::TaskRecord {
        title: "completed bridge".to_string(),
        status: TaskStatus::Done,
        blocked_by: stored_blocked_by(&["FOO-0001"]),
        ..task_record("FOO-0002")
    };
    let hidden_descendant = pwf_wire::task::TaskRecord {
        title: "active behind hidden bridge".to_string(),
        blocked_by: stored_blocked_by(&["FOO-0002"]),
        ..task_record("FOO-0003")
    };
    let visible = pwf_wire::task::TaskRecord {
        title: "active direct dependent".to_string(),
        blocked_by: stored_blocked_by(&["FOO-0001"]),
        ..task_record("FOO-0004")
    };
    let store = InMemoryStore::default()
        .with_project("foo", vec![root, hidden, hidden_descendant, visible]);

    let graph = get_task_dag::execute(
        &GetTaskDag {
            id: TaskId::try_new("FOO-0001").unwrap(),
            depth: None,
            status: StatusFilter::Exact(TaskStatus::Active),
            mode: TaskDagMode::Blocks,
        },
        &store,
        &pool,
    )
    .await
    .unwrap();

    assert_eq!(
        graph
            .nodes()
            .iter()
            .filter_map(TaskDagNode::task_id)
            .map(AsRef::as_ref)
            .collect::<Vec<_>>(),
        ["FOO-0001", "FOO-0004"]
    );
    assert_eq!(
        graph.edges(),
        [TaskDagEdge {
            blocker_node_index: 0,
            dependent_node_index: 1,
        }]
    );
    pool.close().await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn depth_limit_marks_a_hidden_upstream_layer(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    let root = pwf_wire::task::TaskRecord {
        blocked_by: stored_blocked_by(&["FOO-0002"]),
        ..task_record("FOO-0003")
    };
    let direct = pwf_wire::task::TaskRecord {
        blocked_by: stored_blocked_by(&["FOO-0001"]),
        ..task_record("FOO-0002")
    };
    let hidden = task_record("FOO-0001");
    let store = InMemoryStore::default().with_project("foo", vec![root, direct, hidden]);

    let graph = get_task_dag::execute(
        &GetTaskDag {
            id: TaskId::try_new("FOO-0003").unwrap(),
            depth: Some(TaskDagDepth::try_new(1).unwrap()),
            status: StatusFilter::All,
            mode: TaskDagMode::BlockedBy,
        },
        &store,
        &pool,
    )
    .await
    .unwrap();

    assert!(matches!(graph.nodes()[2], TaskDagNode::DepthLimit));
    assert_eq!(
        graph.edges(),
        [
            TaskDagEdge {
                blocker_node_index: 1,
                dependent_node_index: 0,
            },
            TaskDagEdge {
                blocker_node_index: 2,
                dependent_node_index: 1,
            },
        ]
    );
    pool.close().await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn missing_blocker_is_a_terminal_diagnostic_node(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    let root = pwf_wire::task::TaskRecord {
        blocked_by: stored_blocked_by(&["FOO-0002"]),
        ..task_record("FOO-0001")
    };
    let store = InMemoryStore::default().with_project("foo", vec![root]);

    let graph = get_task_dag::execute(
        &GetTaskDag {
            id: TaskId::try_new("FOO-0001").unwrap(),
            depth: None,
            status: StatusFilter::All,
            mode: TaskDagMode::BlockedBy,
        },
        &store,
        &pool,
    )
    .await
    .unwrap();

    assert!(matches!(
        &graph.nodes()[1],
        TaskDagNode::Missing { id } if id.as_ref() == "FOO-0002"
    ));
    assert_eq!(
        graph.edges(),
        [TaskDagEdge {
            blocker_node_index: 1,
            dependent_node_index: 0,
        }]
    );
    pool.close().await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn malformed_blocked_by_keeps_the_task_and_stops_its_branch(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    let root = pwf_wire::task::TaskRecord {
        blocked_by: stored_blocked_by(&["FOO-0002"]),
        ..task_record("FOO-0001")
    };
    let malformed = pwf_wire::task::TaskRecord {
        blocked_by: StoredBlockedBy::Malformed {
            raw: "[[FOO-0003]]".to_string(),
            reason: "expected a YAML sequence".to_string(),
        },
        ..task_record("FOO-0002")
    };
    let store = InMemoryStore::default().with_project("foo", vec![root, malformed]);

    let graph = get_task_dag::execute(
        &GetTaskDag {
            id: TaskId::try_new("FOO-0001").unwrap(),
            depth: None,
            status: StatusFilter::All,
            mode: TaskDagMode::BlockedBy,
        },
        &store,
        &pool,
    )
    .await
    .unwrap();

    assert_eq!(graph.nodes().len(), 2);
    assert!(matches!(
        &graph.nodes()[1],
        TaskDagNode::Task { id, .. } if id.as_ref() == "FOO-0002"
    ));
    pool.close().await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn full_mode_does_not_switch_directions_at_surrounding_nodes(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    let blocker = task_record("FOO-0001");
    let root = pwf_wire::task::TaskRecord {
        blocked_by: stored_blocked_by(&["FOO-0001"]),
        ..task_record("FOO-0002")
    };
    let blocker_sibling = pwf_wire::task::TaskRecord {
        blocked_by: stored_blocked_by(&["FOO-0001"]),
        ..task_record("FOO-0003")
    };
    let dependent = pwf_wire::task::TaskRecord {
        blocked_by: stored_blocked_by(&["FOO-0002"]),
        ..task_record("FOO-0004")
    };
    let store = InMemoryStore::default()
        .with_project("foo", vec![blocker, root, blocker_sibling, dependent]);

    let graph = get_task_dag::execute(
        &GetTaskDag {
            id: TaskId::try_new("FOO-0002").unwrap(),
            depth: None,
            status: StatusFilter::All,
            mode: TaskDagMode::Full,
        },
        &store,
        &pool,
    )
    .await
    .unwrap();

    assert_eq!(
        graph
            .nodes()
            .iter()
            .filter_map(TaskDagNode::task_id)
            .map(AsRef::as_ref)
            .collect::<Vec<_>>(),
        ["FOO-0002", "FOO-0001", "FOO-0004"]
    );
    pool.close().await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn reachable_cycle_is_rejected(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    let first = pwf_wire::task::TaskRecord {
        blocked_by: stored_blocked_by(&["FOO-0002"]),
        ..task_record("FOO-0001")
    };
    let second = pwf_wire::task::TaskRecord {
        blocked_by: stored_blocked_by(&["FOO-0001"]),
        ..task_record("FOO-0002")
    };
    let store = InMemoryStore::default().with_project("foo", vec![first, second]);

    let error = get_task_dag::execute(
        &GetTaskDag {
            id: TaskId::try_new("FOO-0001").unwrap(),
            depth: None,
            status: StatusFilter::All,
            mode: TaskDagMode::BlockedBy,
        },
        &store,
        &pool,
    )
    .await
    .unwrap_err();

    assert!(matches!(error, get_task_dag::GetTaskDagError::Cycle { .. }));
    pool.close().await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn blocks_mode_fails_when_a_project_cannot_be_scanned(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    let store = InMemoryStore::default()
        .with_project("foo", vec![task_record("FOO-0001")])
        .with_failure(InMemoryStoreFailure::ListTasks);

    let error = get_task_dag::execute(
        &GetTaskDag {
            id: TaskId::try_new("FOO-0001").unwrap(),
            depth: None,
            status: StatusFilter::All,
            mode: TaskDagMode::Blocks,
        },
        &store,
        &pool,
    )
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        get_task_dag::GetTaskDagError::ListProjectTasks { .. }
    ));
    pool.close().await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn node_limit_rejects_a_partial_graph(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    let blockers = (2..=513)
        .map(|number| TaskId::try_new(format!("FOO-{number:04}")).unwrap())
        .collect::<Vec<_>>();
    let root = pwf_wire::task::TaskRecord {
        blocked_by: StoredBlockedBy::Valid(BlockedBy::try_new(blockers).unwrap()),
        ..task_record("FOO-0001")
    };
    let store = InMemoryStore::default().with_project("foo", vec![root]);

    let error = get_task_dag::execute(
        &GetTaskDag {
            id: TaskId::try_new("FOO-0001").unwrap(),
            depth: None,
            status: StatusFilter::All,
            mode: TaskDagMode::BlockedBy,
        },
        &store,
        &pool,
    )
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        get_task_dag::GetTaskDagError::NodeLimit { max: 512 }
    ));
    pool.close().await;
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn edge_limit_rejects_a_partial_graph(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
    let tasks = (1..=66)
        .map(|number| {
            let mut task = task_record(&format!("FOO-{number:04}"));
            if number > 1 {
                let blockers = (1..number)
                    .map(|blocker| TaskId::try_new(format!("FOO-{blocker:04}")).unwrap())
                    .collect::<Vec<_>>();
                task.blocked_by = StoredBlockedBy::Valid(BlockedBy::try_new(blockers).unwrap());
            }
            task
        })
        .collect();
    let store = InMemoryStore::default().with_project("foo", tasks);

    let error = get_task_dag::execute(
        &GetTaskDag {
            id: TaskId::try_new("FOO-0066").unwrap(),
            depth: None,
            status: StatusFilter::All,
            mode: TaskDagMode::BlockedBy,
        },
        &store,
        &pool,
    )
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        get_task_dag::GetTaskDagError::EdgeLimit { max: 2_048 }
    ));
    pool.close().await;
}
