use pwf_application::{
    ports::task_vault::{TaskGraphSnapshot, TaskVault},
    project::get_project,
    task::{
        get_task_dag::{self, GetTaskDagError},
        read_task_dependencies::{self, ReadTaskDependencies},
    },
};
use pwf_models::{project::HomeDirectory, task::BlockedBy};
use pwf_wire::task::{GetTaskDag, StatusFilter, TaskDagMode, TaskDagNode};

use super::{ObsidianStore, ObsidianStoreError};
use crate::{database, obsidian::identity::task_directory_scan_count};

struct Fixture {
    directory: tempfile::TempDir,
    pool: sqlx::SqlitePool,
    store: ObsidianStore,
}

impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let pool = database::build_migration_pool(&directory.path().join("registry.sqlite3"))
            .await
            .unwrap();
        database::migrate_database(&pool).await.unwrap();
        for id in ["FOO", "AUX", "BAD"] {
            let path = directory.path().join(id);
            std::fs::create_dir(&path).unwrap();
            sqlx::query("INSERT INTO projects (id, title, tasks_kind, tasks_path, paused_at) VALUES (?, ?, 'directory', ?, ?)")
                .bind(id).bind(id.to_lowercase()).bind(path.to_string_lossy().as_ref())
                .bind((id == "AUX").then_some("2026-09-13T00:00:00Z"))
                .execute(&pool).await.unwrap();
        }
        let store =
            ObsidianStore::with_watched_tasks(HomeDirectory::new(directory.path().to_path_buf()));
        let fixture = Self {
            directory,
            pool,
            store,
        };
        fixture.write("FOO-0001", "");
        fixture.write("FOO-0002", "blocked_by: [\"[[FOO-0001]]\"]\n");
        fixture.write("AUX-0001", "blocked_by: [\"[[FOO-0001]]\"]\n");
        fixture.write(
            "FOO-0003",
            "blocked_by: [\"[[FOO-0002]]\", \"[[AUX-0001]]\"]\n",
        );
        fixture
    }

    fn write(&self, id: &str, metadata: &str) {
        let (project, _) = id.split_once('-').unwrap();
        let mut bytes = format!("---\nid: {id}\ntitle: {id}\n{metadata}---\n").into_bytes();
        bytes.extend_from_slice(b"\xff body is not UTF-8");
        std::fs::write(
            self.directory.path().join(project).join(format!("{id}.md")),
            bytes,
        )
        .unwrap();
    }

    fn query(id: &str, mode: TaskDagMode) -> GetTaskDag {
        GetTaskDag {
            id: id.parse().unwrap(),
            depth: None,
            status: StatusFilter::All,
            mode,
        }
    }
}

#[tokio::test]
async fn dependency_gather_scans_each_reached_project_once_and_returns_only_reached_tasks() {
    let fixture = Fixture::new().await;
    fixture.write("FOO-0098", "status: invalid\nblocked_by: invalid\n");
    std::fs::write(
        fixture.directory.path().join("BAD/broken.md"),
        "---\nid: invalid\n---\n",
    )
    .unwrap();
    let target = "FOO-0099".parse().unwrap();
    let blockers = BlockedBy::try_new(vec!["FOO-0003".parse().unwrap()]).unwrap();
    for _ in 0..2 {
        let before = task_directory_scan_count();
        let records = read_task_dependencies::execute(
            ReadTaskDependencies {
                target: &target,
                blockers: &blockers,
            },
            &fixture.store,
            &fixture.pool,
        )
        .await
        .unwrap();
        assert_eq!(task_directory_scan_count() - before, 2);
        assert_eq!(records.len(), 4);
        assert!(!records.contains_key(&"FOO-0098".parse().unwrap()));
    }
    fixture.pool.close().await;
}

#[tokio::test]
async fn graph_modes_scan_each_required_project_once_including_the_root() {
    let mut fixture = Fixture::new().await;
    fixture.store = ObsidianStore::new(HomeDirectory::new(fixture.directory.path().to_path_buf()));
    for _ in 0..2 {
        for (mode, id, scans) in [
            (TaskDagMode::BlockedBy, "FOO-0003", 2),
            (TaskDagMode::Blocks, "FOO-0001", 3),
            (TaskDagMode::Full, "FOO-0003", 3),
        ] {
            let before = task_directory_scan_count();
            let graph =
                get_task_dag::execute(&Fixture::query(id, mode), &fixture.store, &fixture.pool)
                    .await
                    .unwrap();
            assert_eq!(task_directory_scan_count() - before, scans);
            assert_eq!(graph.nodes().len(), 4);
            assert_eq!(graph.edges().len(), 4);
        }
    }
    fixture.pool.close().await;
}

#[tokio::test]
async fn graph_modes_reuse_cached_projects_across_requests() {
    let fixture = Fixture::new().await;
    for (mode, id, scans) in [
        (TaskDagMode::BlockedBy, "FOO-0003", 2),
        (TaskDagMode::Blocks, "FOO-0001", 1),
        (TaskDagMode::Full, "FOO-0003", 0),
        (TaskDagMode::BlockedBy, "FOO-0003", 0),
    ] {
        let before = task_directory_scan_count();
        let graph = get_task_dag::execute(&Fixture::query(id, mode), &fixture.store, &fixture.pool)
            .await
            .unwrap();
        assert_eq!(task_directory_scan_count() - before, scans);
        assert_eq!(graph.nodes().len(), 4);
        assert_eq!(graph.edges().len(), 4);
    }
    fixture.pool.close().await;
}

#[tokio::test]
async fn list_and_small_graph_share_the_same_1024_task_snapshot() {
    let fixture = Fixture::new().await;
    fixture.write("FOO-0003", "blocked_by: [\"[[FOO-0002]]\"]\n");
    for number in 4..=1024 {
        fixture.write(&format!("FOO-{number:04}"), "");
    }
    let project = get_project::execute(
        pwf_wire::project::GetProject::new(
            "FOO".parse::<pwf_models::project::ProjectId>().unwrap(),
            pwf_wire::project::ProjectStatusFilter::IncludingPaused,
        ),
        &fixture.pool,
    )
    .await
    .unwrap();
    let before = task_directory_scan_count();
    assert_eq!(
        fixture.store.list_task_summaries(&project).unwrap().len(),
        1024
    );
    assert_eq!(task_directory_scan_count() - before, 1);
    let snapshot = fixture.store.list_task_graph_records(&project).unwrap();
    let before = task_directory_scan_count();
    for _ in 0..3 {
        let graph = get_task_dag::execute(
            &Fixture::query("FOO-0003", TaskDagMode::BlockedBy),
            &fixture.store,
            &fixture.pool,
        )
        .await
        .unwrap();
        assert_eq!(graph.nodes().len(), 3);
        let next = fixture.store.list_task_graph_records(&project).unwrap();
        assert!(std::sync::Arc::ptr_eq(&snapshot.files, &next.files));
    }
    assert_eq!(task_directory_scan_count(), before);

    fixture
        .store
        .invalidate_task_index(&fixture.directory.path().join("FOO"));
    let next = fixture.store.list_task_graph_records(&project).unwrap();
    assert!(!std::sync::Arc::ptr_eq(&snapshot.files, &next.files));
    let before = task_directory_scan_count();
    assert_eq!(
        fixture.store.list_task_summaries(&project).unwrap().len(),
        1024
    );
    assert_eq!(task_directory_scan_count(), before);
    fixture.pool.close().await;
}

#[tokio::test]
async fn external_edges_refresh_graph_cache_while_mutation_reads_are_immediate() {
    let fixture = Fixture::new().await;
    let query = Fixture::query("FOO-0003", TaskDagMode::BlockedBy);
    let first = get_task_dag::execute(&query, &fixture.store, &fixture.pool)
        .await
        .unwrap();
    assert_eq!(first.nodes().len(), 4);
    fixture.write("FOO-0003", "created_at: invalid\ncompleted_at: invalid\n");
    let target = "FOO-0099".parse().unwrap();
    let blockers = BlockedBy::try_new(vec![query.id.clone()]).unwrap();
    let records = read_task_dependencies::execute(
        ReadTaskDependencies {
            target: &target,
            blockers: &blockers,
        },
        &fixture.store,
        &fixture.pool,
    )
    .await
    .unwrap();
    assert_eq!(records.len(), 1);
    let refreshed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while get_task_dag::execute(&query, &fixture.store, &fixture.pool)
            .await
            .unwrap()
            .nodes()
            .len()
            != 1
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(
        refreshed.is_ok(),
        "external dependency edit was not observed"
    );
    fixture.pool.close().await;
}

#[tokio::test]
async fn graph_field_errors_apply_to_the_root_reached_task_or_reverse_scan() {
    let fixture = Fixture::new().await;
    fixture.write("FOO-0098", "status: invalid\n");
    let query = Fixture::query("FOO-0003", TaskDagMode::BlockedBy);
    assert!(
        get_task_dag::execute(&query, &fixture.store, &fixture.pool)
            .await
            .is_ok()
    );
    fixture.write("FOO-0002", "status: invalid\n");
    fixture
        .store
        .invalidate_task_index(&fixture.directory.path().join("FOO"));
    let graph = get_task_dag::execute(&query, &fixture.store, &fixture.pool)
        .await
        .unwrap();
    assert!(
        graph.nodes().iter().any(
            |node| matches!(node, TaskDagNode::Unavailable { id } if id.as_ref() == "FOO-0002")
        )
    );
    let error = get_task_dag::execute(
        &Fixture::query("FOO-0002", TaskDagMode::BlockedBy),
        &fixture.store,
        &fixture.pool,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, GetTaskDagError::ReadRoot { .. }));
    let error = get_task_dag::execute(
        &Fixture::query("FOO-0003", TaskDagMode::Full),
        &fixture.store,
        &fixture.pool,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, GetTaskDagError::ListProjectTasks { .. }));
    fixture.pool.close().await;
}

#[tokio::test]
async fn graph_ignores_cached_summary_errors_and_retains_graph_errors_by_identity() {
    let fixture = Fixture::new().await;
    fixture.write("FOO-0001", "created_at: invalid\n");
    fixture.write("FOO-0098", "status: invalid\n");
    let project = get_project::execute(
        pwf_wire::project::GetProject::new(
            "FOO".parse::<pwf_models::project::ProjectId>().unwrap(),
            pwf_wire::project::ProjectStatusFilter::IncludingPaused,
        ),
        &fixture.pool,
    )
    .await
    .unwrap();
    assert!(fixture.store.list_task_summaries(&project).is_err());
    let before = task_directory_scan_count();
    let snapshot = fixture.store.list_task_graph_records(&project).unwrap();
    assert_eq!(task_directory_scan_count(), before);
    assert!(snapshot.get(&"FOO-0001".parse().unwrap()).unwrap().is_ok());
    assert!(
        matches!(snapshot.get(&"FOO-0098".parse().unwrap()).unwrap(), Err(error) if matches!(error.as_ref(), ObsidianStoreError::InvalidTaskStatus { .. }))
    );
    fixture.pool.close().await;
}

#[tokio::test]
async fn snapshots_reject_duplicate_frontmatter_identity() {
    let fixture = Fixture::new().await;
    let project = get_project::execute(
        pwf_wire::project::GetProject::new(
            pwf_models::project::ProjectId::try_new("FOO").unwrap(),
            pwf_wire::project::ProjectStatusFilter::IncludingPaused,
        ),
        &fixture.pool,
    )
    .await
    .unwrap();
    let directory = fixture.directory.path().join("FOO");
    std::fs::copy(
        directory.join("FOO-0001.md"),
        directory.join("different-name.md"),
    )
    .unwrap();
    assert!(matches!(
        fixture.store.list_task_dependencies(&project),
        Err(ObsidianStoreError::DuplicateTaskId { .. })
    ));
    assert!(matches!(
        fixture.store.list_task_graph_records(&project),
        Err(ObsidianStoreError::DuplicateTaskId { .. })
    ));
    fixture.pool.close().await;
}
