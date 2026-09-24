use std::{collections::BTreeSet, time::Duration};

use pwf_application::ports::project_store::ProjectStore;
use pwf_models::project::{
    HomeDirectory, ProjectId, ProjectTasks, ProjectTasksKind, ProjectTasksPath,
};
use pwf_wire::{
    patch_field::PatchField,
    project::{GetProject, ProjectFields, ProjectStatusFilter, RenameProject, UpdateProject},
    set_field::SetField,
};

use super::SqliteProjectStore;

fn home() -> HomeDirectory {
    HomeDirectory::new(std::env::temp_dir().join("pwf-project-store-home"))
}
fn id(raw: &str) -> ProjectId {
    raw.parse().unwrap()
}
fn query(raw: &str) -> GetProject {
    GetProject::new(id(raw), ProjectStatusFilter::IncludingPaused)
}
fn fields(raw: &str) -> ProjectFields {
    ProjectFields {
        id: id(raw),
        title: pwf_models::project::ProjectName::try_new(raw).unwrap(),
        source: None,
        tasks: ProjectTasks::new(
            ProjectTasksKind::Directory,
            ProjectTasksPath::try_new(format!("~/tasks/{raw}")).unwrap(),
        ),
        obsidian_vault: None,
        snapshot_enabled: false,
    }
}
fn update(raw: &str, enabled: bool) -> UpdateProject {
    UpdateProject {
        id: id(raw),
        source: PatchField::NoAction,
        obsidian_vault: PatchField::NoAction,
        snapshot_enabled: SetField::Set(enabled),
    }
}

#[sqlx::test]
async fn reads_share_project_values_and_batch_only_missing_ids(pool: sqlx::SqlitePool) {
    let store = SqliteProjectStore::new(pool);
    store.add_project(fields("FOO"), &home()).await.unwrap();
    store.add_project(fields("BAR"), &home()).await.unwrap();
    let foo = store.get_project(query("FOO")).await.unwrap();
    assert_eq!(store.clone().get_project(query("FOO")).await.unwrap(), foo);
    assert_eq!(store.state.lock().await.reads, 1);
    let ids = BTreeSet::from([id("BAR"), id("FOO")]);
    let projects = store.get_projects(&ids).await.unwrap();
    assert_eq!(
        projects
            .iter()
            .map(|project| &project.id)
            .collect::<Vec<_>>(),
        ids.iter().collect::<Vec<_>>()
    );
    assert_eq!(store.state.lock().await.reads, 2);
    assert_eq!(store.get_projects(&ids).await.unwrap(), projects);
    assert_eq!(store.state.lock().await.reads, 2);
    let all = store
        .list_projects(ProjectStatusFilter::IncludingPaused)
        .await
        .unwrap();
    assert_eq!(
        store
            .list_projects(ProjectStatusFilter::ActiveOnly)
            .await
            .unwrap(),
        all
    );
    assert_eq!(store.get_projects(&ids).await.unwrap(), projects);
    assert_eq!(store.state.lock().await.reads, 3);
}

#[sqlx::test]
async fn all_mutations_invalidate_lists_and_individual_reads(pool: sqlx::SqlitePool) {
    let store = SqliteProjectStore::new(pool);
    assert!(
        store
            .list_projects(ProjectStatusFilter::IncludingPaused)
            .await
            .unwrap()
            .is_empty()
    );
    store.add_project(fields("FOO"), &home()).await.unwrap();
    assert_eq!(
        store
            .list_projects(ProjectStatusFilter::ActiveOnly)
            .await
            .unwrap()
            .len(),
        1
    );
    store.update_project(update("FOO", true)).await.unwrap();
    assert!(
        store
            .get_project(query("FOO"))
            .await
            .unwrap()
            .snapshot_enabled
    );
    store.pause_project(id("FOO")).await.unwrap();
    assert!(store.get_project(query("FOO")).await.unwrap().is_paused);
    assert!(
        store
            .get_project(GetProject::new(id("FOO"), ProjectStatusFilter::ActiveOnly))
            .await
            .is_err()
    );
    assert!(
        store
            .list_projects(ProjectStatusFilter::ActiveOnly)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .list_projects(ProjectStatusFilter::IncludingPaused)
            .await
            .unwrap()
            .len(),
        1
    );
    store.resume_project(id("FOO"), &home()).await.unwrap();
    let current = store.get_project(query("FOO")).await.unwrap();
    assert!(!current.is_paused);
    let renamed = store
        .rename_project(
            RenameProject {
                current_id: id("FOO"),
                fields: fields("BAR"),
            },
            &current,
            &home(),
        )
        .await
        .unwrap();
    assert!(store.get_project(query("FOO")).await.is_err());
    assert_eq!(store.get_project(query("BAR")).await.unwrap(), renamed);
    // A compensating rename is another committed registry mutation.
    store
        .rename_project(
            RenameProject {
                current_id: id("BAR"),
                fields: fields("FOO"),
            },
            &renamed,
            &home(),
        )
        .await
        .unwrap();
    assert!(store.get_project(query("BAR")).await.is_err());
    assert_eq!(
        store
            .list_projects(ProjectStatusFilter::IncludingPaused)
            .await
            .unwrap()[0]
            .id,
        id("FOO")
    );
}

#[sqlx::test]
async fn concurrent_cold_reads_execute_one_query(pool: sqlx::SqlitePool) {
    let store = SqliteProjectStore::new(pool);
    store.add_project(fields("FOO"), &home()).await.unwrap();
    let mut readers = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let store = store.clone();
        readers.spawn(async move { store.get_project(query("FOO")).await.unwrap() });
    }
    while let Some(result) = readers.join_next().await {
        assert_eq!(result.unwrap().id, id("FOO"));
    }
    assert_eq!(store.state.lock().await.reads, 1);
}

#[sqlx::test]
async fn failed_writes_and_cancelled_transactions_leave_reads_current(pool: sqlx::SqlitePool) {
    let store = SqliteProjectStore::new(pool.clone());
    store.add_project(fields("FOO"), &home()).await.unwrap();
    let original = store.get_project(query("FOO")).await.unwrap();
    assert!(store.add_project(fields("FOO"), &home()).await.is_err());
    assert_eq!(store.get_project(query("FOO")).await.unwrap(), original);
    let transaction = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(20),
            store.update_project(update("FOO", true))
        )
        .await
        .is_err()
    );
    transaction.rollback().await.unwrap();
    let observed = tokio::time::timeout(Duration::from_secs(5), store.get_project(query("FOO")))
        .await
        .unwrap()
        .unwrap();
    let committed: bool =
        sqlx::query_scalar("SELECT snapshot_enabled FROM projects WHERE id = 'FOO'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(observed.snapshot_enabled, committed);
    assert_eq!(store.get_project(query("FOO")).await.unwrap(), observed);
    store.update_project(update("FOO", true)).await.unwrap();
    assert!(
        store
            .get_project(query("FOO"))
            .await
            .unwrap()
            .snapshot_enabled
    );
}

#[sqlx::test]
async fn unrelated_invalid_rows_do_not_poison_selective_reads(pool: sqlx::SqlitePool) {
    let store = SqliteProjectStore::new(pool.clone());
    store.add_project(fields("FOO"), &home()).await.unwrap();
    store.add_project(fields("BAD"), &home()).await.unwrap();
    sqlx::query("UPDATE projects SET created_at = 'invalid' WHERE id = 'BAD'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        store
            .list_projects(ProjectStatusFilter::IncludingPaused)
            .await
            .is_err()
    );
    assert_eq!(store.get_project(query("FOO")).await.unwrap().id, id("FOO"));
    let projects = store
        .get_projects(&BTreeSet::from([id("FOO"), id("MISS")]))
        .await
        .unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, id("FOO"));
}

#[sqlx::test]
async fn source_changes_invalidate_the_joined_projection(pool: sqlx::SqlitePool) {
    use pwf_models::project::{ProjectSource, ProjectSourceKind, ProjectSourceValue};
    let store = SqliteProjectStore::new(pool);
    store.add_project(fields("FOO"), &home()).await.unwrap();
    assert!(
        store
            .get_project(query("FOO"))
            .await
            .unwrap()
            .source
            .is_none()
    );
    let source = ProjectSource::new(
        ProjectSourceKind::Directory,
        ProjectSourceValue::try_new("~/work/foo").unwrap(),
    );
    store
        .update_project(UpdateProject {
            source: PatchField::Set(source.clone()),
            ..update("FOO", true)
        })
        .await
        .unwrap();
    assert_eq!(
        store.get_project(query("FOO")).await.unwrap().source,
        Some(source)
    );
    store
        .update_project(UpdateProject {
            source: PatchField::Clear,
            ..update("FOO", true)
        })
        .await
        .unwrap();
    assert!(
        store
            .list_projects(ProjectStatusFilter::IncludingPaused)
            .await
            .unwrap()[0]
            .source
            .is_none()
    );
}

#[sqlx::test]
async fn task_sequence_reservations_are_unique_across_connections(pool: sqlx::SqlitePool) {
    let first = SqliteProjectStore::new(pool.clone());
    let second = SqliteProjectStore::new(pool);
    first.add_project(fields("FOO"), &home()).await.unwrap();
    assert!(first.reserve_task_id(&id("FOO")).await.unwrap().is_none());
    first
        .advance_task_sequence(&id("FOO"), Some(&"FOO-0005".parse().unwrap()))
        .await
        .unwrap();
    let mut workers = tokio::task::JoinSet::new();
    for index in 0..32 {
        let store = if index % 2 == 0 {
            first.clone()
        } else {
            second.clone()
        };
        workers.spawn(async move {
            // A concurrent seed from an older snapshot must never rewind reservations.
            store
                .advance_task_sequence(&id("FOO"), Some(&"FOO-0003".parse().unwrap()))
                .await
                .unwrap();
            store
                .reserve_task_id(&id("FOO"))
                .await
                .unwrap()
                .unwrap()
                .number()
        });
    }
    let mut allocated = BTreeSet::new();
    while let Some(result) = workers.join_next().await {
        assert!(allocated.insert(result.unwrap()));
    }
    assert_eq!(allocated, (6..38).collect());
    first.add_project(fields("BAR"), &home()).await.unwrap();
    first.advance_task_sequence(&id("BAR"), None).await.unwrap();
    assert_eq!(
        first
            .reserve_task_id(&id("BAR"))
            .await
            .unwrap()
            .unwrap()
            .as_ref(),
        "BAR-0001"
    );
}

#[sqlx::test]
async fn task_sequence_survives_rename_and_stops_at_the_id_limit(pool: sqlx::SqlitePool) {
    use pwf_application::ports::project_store::TaskSequenceError;
    let store = SqliteProjectStore::new(pool);
    let original = store.add_project(fields("FOO"), &home()).await.unwrap();
    store
        .advance_task_sequence(&id("FOO"), Some(&"FOO-9998".parse().unwrap()))
        .await
        .unwrap();
    store
        .rename_project(
            RenameProject {
                current_id: id("FOO"),
                fields: fields("BAR"),
            },
            &original,
            &home(),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .reserve_task_id(&id("BAR"))
            .await
            .unwrap()
            .unwrap()
            .as_ref(),
        "BAR-9999"
    );
    assert!(matches!(
        store.reserve_task_id(&id("BAR")).await,
        Err(TaskSequenceError::Exhausted { .. })
    ));
    assert!(matches!(
        store.reserve_task_id(&id("FOO")).await,
        Err(TaskSequenceError::ProjectNotFound { .. })
    ));
    store.advance_task_sequence(&id("BAR"), None).await.unwrap();
    assert!(matches!(
        store.reserve_task_id(&id("BAR")).await,
        Err(TaskSequenceError::Exhausted { .. })
    ));
}

#[tokio::test]
async fn creation_seeds_once_and_reopened_stores_do_not_scan_or_reuse_ids() {
    use pwf_application::task::add_task;
    use pwf_wire::task::{AddTask, AddTaskBody};

    use crate::{
        clock::LocalClock, database, obsidian::ObsidianStore, user_settings::TomlSettingsStore,
    };
    let directory = tempfile::tempdir().unwrap();
    let tasks = directory.path().join("tasks");
    std::fs::create_dir(&tasks).unwrap();
    for number in [5, 3, 2, 1] {
        std::fs::write(
            tasks.join(format!("authored-{number}.md")),
            format!("---\nid: FOO-{number:04}\ntitle: Existing\n---\nbody\n"),
        )
        .unwrap();
    }
    let database = directory.path().join("projects.sqlite3");
    let pool = database::build_pool(&database).await.unwrap();
    database::migrate_database(&pool).await.unwrap();
    let projects = SqliteProjectStore::new(pool.clone());
    projects
        .add_project(
            ProjectFields {
                tasks: ProjectTasks::new(
                    ProjectTasksKind::Directory,
                    ProjectTasksPath::try_new(tasks.to_string_lossy()).unwrap(),
                ),
                ..fields("FOO")
            },
            &home(),
        )
        .await
        .unwrap();
    let store = ObsidianStore::new(HomeDirectory::new(directory.path().to_path_buf()));
    let settings = TomlSettingsStore::new(None);
    let command = || AddTask::new(id("FOO"), AddTaskBody::from_shorthand("Create a task"));
    let scans = crate::obsidian::task_directory_scan_count();
    let result = add_task::execute(command(), &store, &projects, &LocalClock, &settings)
        .await
        .unwrap();
    assert_eq!(result.outcome.as_ref(), "FOO-0006");
    assert_eq!(crate::obsidian::task_directory_scan_count() - scans, 1);
    std::fs::remove_file(tasks.join("FOO-0006.md")).unwrap();
    drop((store, projects));
    pool.close().await;

    let pool = database::build_pool(&database).await.unwrap();
    let projects = SqliteProjectStore::new(pool.clone());
    let store = ObsidianStore::new(HomeDirectory::new(directory.path().to_path_buf()));
    let settings = TomlSettingsStore::new(None);
    let scans = crate::obsidian::task_directory_scan_count();
    let result = add_task::execute(command(), &store, &projects, &LocalClock, &settings)
        .await
        .unwrap();
    assert_eq!(result.outcome.as_ref(), "FOO-0007");
    assert_eq!(crate::obsidian::task_directory_scan_count(), scans);

    // Publishing fails after reservation; the subsequent create still advances.
    std::fs::create_dir(tasks.join("FOO-0008.md")).unwrap();
    assert!(
        add_task::execute(command(), &store, &projects, &LocalClock, &settings)
            .await
            .is_err()
    );
    let result = add_task::execute(command(), &store, &projects, &LocalClock, &settings)
        .await
        .unwrap();
    assert_eq!(result.outcome.as_ref(), "FOO-0009");
    assert_eq!(crate::obsidian::task_directory_scan_count(), scans);
    assert!(tasks.join("FOO-0008.md").is_dir());

    std::fs::remove_dir(tasks.join("FOO-0008.md")).unwrap();

    // Explicit reconciliation supports externally imported IDs without lowering the sequence.
    let project = projects.get_project(query("FOO")).await.unwrap();
    std::fs::write(
        tasks.join("imported.md"),
        "---\nid: FOO-0100\ntitle: Imported\n---\nbody\n",
    )
    .unwrap();
    let highest =
        pwf_application::ports::task_vault::TaskVault::highest_task_id(&store, &project).unwrap();
    projects
        .advance_task_sequence(&project.id, highest.as_ref())
        .await
        .unwrap();
    let result = add_task::execute(command(), &store, &projects, &LocalClock, &settings)
        .await
        .unwrap();
    assert_eq!(result.outcome.as_ref(), "FOO-0101");
}

#[sqlx::test]
async fn failed_reservation_commit_does_not_return_an_id(pool: sqlx::SqlitePool) {
    let store = SqliteProjectStore::new(pool.clone());
    store.add_project(fields("FOO"), &home()).await.unwrap();
    store.advance_task_sequence(&id("FOO"), None).await.unwrap();
    sqlx::raw_sql(
        "CREATE TABLE reservation_guard (project_id TEXT REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED);
         CREATE TRIGGER fail_reservation AFTER UPDATE OF last_task_number ON projects
         BEGIN INSERT INTO reservation_guard VALUES ('MISSING'); END;",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(store.reserve_task_id(&id("FOO")).await.is_err());
    sqlx::query("DROP TRIGGER fail_reservation")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        store
            .reserve_task_id(&id("FOO"))
            .await
            .unwrap()
            .unwrap()
            .as_ref(),
        "FOO-0001"
    );
}
