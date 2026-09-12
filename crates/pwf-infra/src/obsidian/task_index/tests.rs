use std::{cell::Cell, fs, io, panic::AssertUnwindSafe};

use notify::event::{DataChange, Flag, MetadataKind, RenameMode};
use pwf_application::ports::task_vault::TaskSummaryRecord;
use pwf_models::task::{TaskId, TaskStatus};
use pwf_wire::task::RawTaskTags;

use super::*;

fn directory(parent: &Path, name: &str) -> PathBuf {
    let directory = parent.join(name);
    fs::create_dir_all(&directory).unwrap();
    directory
}

fn task(directory: &Path, number: u16, title_bytes: usize) -> TaskFile {
    let raw_id = format!("FOO-{number:04}");
    let id = TaskId::try_new(raw_id.as_str()).unwrap();
    TaskFile {
        path: directory.join(format!("{id}.md")).as_path().to_path_buf(),
        summary: Some(TaskSummaryRecord {
            id: id.clone(),
            title: "x".repeat(title_bytes).into_boxed_str().into_string(),
            status: TaskStatus::Active,
            created_at: Some("2026-01-01T00:00:00Z".parse().unwrap()),
            effort: None,
            priority: None,
            tags: None,
        }),
        id,
    }
}

fn summaries(directory: &Path, title_bytes: usize) -> Vec<TaskFile> {
    vec![task(directory, 1, title_bytes)]
}

fn unexpected_scan() -> Result<Vec<TaskFile>, ObsidianStoreError> {
    Err(ObsidianStoreError::ReadTaskFile {
        source: io::Error::other("cached read performed a new scan"),
    })
}

fn modified() -> Event {
    Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
        .add_path(PathBuf::from("vault/tasks/FOO-0001.md"))
}

fn watch_users(state: &IndexState, path: &Path) -> Option<usize> {
    state
        .watches
        .iter()
        .find_map(|(watched, users)| (watched == path).then_some(*users))
}

#[test]
fn exact_budget_includes_both_arc_headers_and_reserved_capacity() {
    let root = tempfile::tempdir().unwrap();
    let directory = directory(root.path(), "tasks");
    let index = TaskIndex::new();
    let overhead = {
        let state = index.state.lock().unwrap();
        size_of::<TaskIndex>()
            + size_of::<Mutex<Changes>>()
            + 4 * size_of::<usize>()
            + state.entries.capacity() * size_of::<Entry>()
            + state.watches.capacity() * size_of::<(PathBuf, usize)>()
    };
    let padding_bytes = INDEX_BYTES_MAX
        - overhead
        - directory.as_os_str().len()
        - root.path().as_os_str().len()
        - entry_bytes(&directory, &summaries(&directory, 0));

    let first = index
        .read(&directory, || Ok(summaries(&directory, padding_bytes)))
        .unwrap();
    let second = index.read(&directory, unexpected_scan).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(
        index.state.lock().unwrap().accounted_bytes(),
        INDEX_BYTES_MAX
    );
    drop((first, second));

    index.invalidate(&directory);
    let fresh = index
        .read(&directory, || Ok(summaries(&directory, padding_bytes + 1)))
        .unwrap();
    assert_eq!(fresh.len(), 1);
    let state = index.state.lock().unwrap();
    assert!(state.entries.is_empty());
    assert!(state.watches.is_empty());
}

#[test]
fn oversized_summaries_and_title_capacity_stay_uncached() {
    let root = tempfile::tempdir().unwrap();
    let retained = directory(root.path(), "retained");
    let oversized = directory(root.path(), "oversized");
    let index = TaskIndex::new();
    index
        .read(&retained, || Ok(summaries(&retained, 0)))
        .unwrap();

    let large = index
        .read(&oversized, || {
            Ok(vec![
                task(&oversized, 1, INDEX_BYTES_MAX / 2),
                task(&oversized, 2, INDEX_BYTES_MAX / 2),
            ])
        })
        .unwrap();
    assert_eq!(large.len(), 2);
    let large_title = index
        .read(&oversized, || {
            let mut note = task(&oversized, 1, INDEX_BYTES_MAX / 2);
            let mut title = String::with_capacity(INDEX_BYTES_MAX);
            title.push('x');
            note.summary.as_mut().unwrap().title = title;
            Ok(vec![note])
        })
        .unwrap();
    assert_eq!(large_title.len(), 1);

    let state = index.state.lock().unwrap();
    assert_eq!(state.entries.len(), 1);
    assert_eq!(state.entries[0].directory, retained);
    assert_eq!(watch_users(&state, root.path()), Some(1));
    assert_eq!(watch_users(&state, &oversized), None);
    assert!(state.accounted_bytes() <= INDEX_BYTES_MAX);
}

#[test]
fn byte_pressure_evicts_oldest_and_releases_its_watch() {
    let root = tempfile::tempdir().unwrap();
    let directories: Vec<_> = (0..3)
        .map(|number| directory(root.path(), &format!("tasks-{number}")))
        .collect();
    let index = TaskIndex::new();
    for directory in &directories {
        index
            .read(directory, || Ok(summaries(directory, 400 * 1024)))
            .unwrap();
        assert!(index.state.lock().unwrap().accounted_bytes() <= INDEX_BYTES_MAX);
    }
    let state = index.state.lock().unwrap();
    assert_eq!(state.entries.len(), 2);
    assert_eq!(state.entries[0].directory, directories[1]);
    assert_eq!(state.entries[1].directory, directories[2]);
    assert_eq!(watch_users(&state, &directories[0]), None);
    assert_eq!(watch_users(&state, root.path()), Some(2));
}

#[test]
fn directory_pressure_caps_watches_and_hits_do_not_change_eviction_order() {
    let root = tempfile::tempdir().unwrap();
    let directories: Vec<_> = (0..=DIRECTORY_COUNT_MAX)
        .map(|number| directory(root.path(), &format!("parent-{number}/tasks")))
        .collect();
    let index = TaskIndex::new();
    for directory in &directories[..DIRECTORY_COUNT_MAX] {
        index
            .read(directory, || Ok(summaries(directory, 0)))
            .unwrap();
    }
    index.read(&directories[0], unexpected_scan).unwrap();
    assert_eq!(index.state.lock().unwrap().watches.len(), WATCH_COUNT_MAX);

    let newest = &directories[DIRECTORY_COUNT_MAX];
    index.read(newest, || Ok(summaries(newest, 0))).unwrap();
    let state = index.state.lock().unwrap();
    assert_eq!(state.entries.len(), DIRECTORY_COUNT_MAX);
    assert_eq!(state.watches.len(), WATCH_COUNT_MAX);
    assert_eq!(state.entries[0].directory, directories[1]);
    assert_eq!(watch_users(&state, &directories[0]), None);
    assert_eq!(watch_users(&state, directories[0].parent().unwrap()), None);
    assert!(state.accounted_bytes() <= INDEX_BYTES_MAX);
}

#[test]
fn own_invalidation_preserves_a_directory_watched_as_another_entries_parent() {
    let root = tempfile::tempdir().unwrap();
    let parent = directory(root.path(), "tasks");
    let child = directory(&parent, "child");
    let index = TaskIndex::new();
    index.read(&parent, || Ok(summaries(&parent, 0))).unwrap();
    let before = index.read(&child, || Ok(summaries(&child, 0))).unwrap();
    assert_eq!(watch_users(&index.state.lock().unwrap(), &parent), Some(2));

    index.invalidate(&parent);
    assert_eq!(watch_users(&index.state.lock().unwrap(), &parent), Some(1));
    let after = index.read(&child, unexpected_scan).unwrap();
    assert!(Arc::ptr_eq(&before, &after));
    let scans = Cell::new(0);
    index
        .read(&parent, || {
            scans.set(scans.get() + 1);
            Ok(summaries(&parent, 0))
        })
        .unwrap();
    assert_eq!(scans.get(), 1);

    index.invalidate(&child);
    assert_eq!(watch_users(&index.state.lock().unwrap(), &parent), Some(1));
    index.invalidate(&parent);
    let state = index.state.lock().unwrap();
    assert!(state.entries.is_empty());
    assert!(state.watches.is_empty());
    assert!(state.watcher.is_some());
}

#[test]
fn unavailable_watcher_always_scans_fresh() {
    let root = tempfile::tempdir().unwrap();
    let index = TaskIndex::new();
    index.state.lock().unwrap().disable();
    let scans = Cell::new(0);
    for _ in 0..2 {
        index
            .read(root.path(), || {
                scans.set(scans.get() + 1);
                Ok(summaries(root.path(), 0))
            })
            .unwrap();
    }
    assert_eq!(scans.get(), 2);
    let state = index.state.lock().unwrap();
    assert!(state.entries.is_empty());
    assert!(state.watches.is_empty());
}

#[test]
fn scan_errors_release_watches_and_are_retried() {
    let root = tempfile::tempdir().unwrap();
    let directory = directory(root.path(), "tasks");
    let index = TaskIndex::new();
    let failed = index.read(&directory, || {
        Err(ObsidianStoreError::ReadTaskFile {
            source: io::Error::new(io::ErrorKind::PermissionDenied, "blocked"),
        })
    });
    assert!(
        matches!(failed, Err(ObsidianStoreError::ReadTaskFile { source })
        if source.kind() == io::ErrorKind::PermissionDenied)
    );
    assert!(index.state.lock().unwrap().entries.is_empty());
    assert!(index.state.lock().unwrap().watches.is_empty());

    index
        .read(&directory, || Ok(summaries(&directory, 0)))
        .unwrap();
    assert_eq!(index.state.lock().unwrap().entries.len(), 1);
}

#[test]
fn watcher_errors_bypass_debounce_and_drop_retained_state() {
    let root = tempfile::tempdir().unwrap();
    let directory = directory(root.path(), "tasks");
    let index = TaskIndex::new();
    let before = index
        .read(&directory, || Ok(summaries(&directory, 0)))
        .unwrap();
    index
        .changes
        .lock()
        .unwrap()
        .observe(Err(notify::Error::io(io::Error::other("watch failed"))));

    let after = index
        .read(&directory, || Ok(summaries(&directory, 0)))
        .unwrap();
    assert!(!Arc::ptr_eq(&before, &after));
    let state = index.state.lock().unwrap();
    assert!(state.entries.is_empty());
    assert!(state.watches.is_empty());
    assert!(state.watcher.is_none());
}

#[test]
fn missing_os_watch_keeps_other_directories_cached_and_watched() {
    let root = tempfile::tempdir().unwrap();
    let directory = directory(root.path(), "tasks");
    let retained = root.path().join("retained");
    fs::create_dir(&retained).unwrap();
    let index = TaskIndex::new();
    index
        .read(&directory, || Ok(summaries(&directory, 0)))
        .unwrap();
    let before = index
        .read(&retained, || Ok(summaries(&retained, 0)))
        .unwrap();
    index
        .state
        .lock()
        .unwrap()
        .watcher
        .as_mut()
        .unwrap()
        .unwatch(&directory)
        .unwrap();

    index.invalidate(&directory);
    let after = index.read(&retained, unexpected_scan).unwrap();
    assert!(Arc::ptr_eq(&before, &after));
    let state = index.state.lock().unwrap();
    assert_eq!(state.entries.len(), 1);
    assert_eq!(state.watches.len(), 2);
    assert_eq!(watch_users(&state, &directory), None);
    assert_eq!(watch_users(&state, root.path()), Some(1));
    assert!(state.watcher.is_some());
}

#[test]
fn summary_optional_string_capacity_and_compact_tags_count_toward_budget() {
    let root = tempfile::tempdir().unwrap();
    let directory = directory(root.path(), "tasks");
    let index = TaskIndex::new();
    let fresh = index
        .read(&directory, || {
            let mut note = task(&directory, 1, 0);
            let summary = note.summary.as_mut().unwrap();
            let mut effort = String::with_capacity(INDEX_BYTES_MAX / 2);
            effort.push_str("small");
            let mut priority = String::with_capacity(INDEX_BYTES_MAX / 2);
            priority.push_str("low");
            summary.effort = Some(effort);
            summary.priority = Some(priority);
            Ok(vec![note])
        })
        .unwrap();
    assert_eq!(fresh.len(), 1);
    assert!(index.state.lock().unwrap().entries.is_empty());

    index
        .read(&directory, || {
            let mut note = task(&directory, 1, 0);
            note.summary.as_mut().unwrap().tags = Some(RawTaskTags::new(
                "x".repeat(INDEX_BYTES_MAX).into_boxed_str().into_string(),
            ));
            Ok(vec![note])
        })
        .unwrap();
    let state = index.state.lock().unwrap();
    assert!(state.entries.is_empty());
    assert!(state.watches.is_empty());
}

#[test]
fn identities_without_parsed_summaries_remain_cacheable() {
    let root = tempfile::tempdir().unwrap();
    let directory = directory(root.path(), "tasks");
    let index = TaskIndex::new();
    let first = index
        .read(&directory, || {
            let mut note = task(&directory, 1, 0);
            note.summary = None;
            Ok(vec![note])
        })
        .unwrap();
    let second = index.read(&directory, unexpected_scan).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert!(second[0].summary.is_none());
    assert_eq!(second[0].id.as_ref(), "FOO-0001");
    assert_eq!(second[0].path, directory.join("FOO-0001.md"));
}

#[test]
fn events_during_scan_prevent_retention_and_force_another_scan() {
    let root = tempfile::tempdir().unwrap();
    let directory = directory(root.path(), "tasks");
    let index = TaskIndex::new();
    let scans = Cell::new(0);
    index
        .read(&directory, || {
            scans.set(scans.get() + 1);
            index.changes.lock().unwrap().observe(Ok(modified()));
            Ok(summaries(&directory, 0))
        })
        .unwrap();
    assert!(index.state.lock().unwrap().entries.is_empty());
    assert!(index.state.lock().unwrap().watches.is_empty());
    index.changes.lock().unwrap().pending_since =
        Some(Instant::now().checked_sub(DEBOUNCE).unwrap());

    index
        .read(&directory, || {
            scans.set(scans.get() + 1);
            Ok(summaries(&directory, 0))
        })
        .unwrap();
    index.read(&directory, unexpected_scan).unwrap();
    assert_eq!(scans.get(), 2);
}

#[test]
fn silent_notification_loss_is_reconciled_after_max_age() {
    let root = tempfile::tempdir().unwrap();
    let directory = directory(root.path(), "tasks");
    let index = TaskIndex::new();
    let before = index
        .read(&directory, || Ok(summaries(&directory, 0)))
        .unwrap();
    index.state.lock().unwrap().entries[0].scanned_at =
        Instant::now().checked_sub(AGE_MAX).unwrap();
    let after = index
        .read(&directory, || Ok(summaries(&directory, 0)))
        .unwrap();
    assert!(!Arc::ptr_eq(&before, &after));
}

#[test]
fn poisoned_cache_falls_back_to_fresh_metadata() {
    let root = tempfile::tempdir().unwrap();
    let directory = directory(root.path(), "tasks");
    let index = TaskIndex::new();
    let panicked = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = index.read(&directory, || {
            std::panic::resume_unwind(Box::new("scan panicked"))
        });
    }));
    assert!(panicked.is_err());
    let fresh = index
        .read(&directory, || Ok(summaries(&directory, 0)))
        .unwrap();
    assert_eq!(fresh.len(), 1);
    let state = index
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(state.entries.is_empty());
    assert!(state.watches.is_empty());
    assert!(state.watcher.is_none());
}

#[test]
fn write_churn_does_not_extend_first_observed_deadline() {
    let index = TaskIndex::new();
    let first = Instant::now().checked_sub(DEBOUNCE).unwrap();
    {
        let mut changes = index.changes.lock().unwrap();
        changes.observe(Ok(modified()));
        changes.pending_since = Some(first);
        for _ in 0..1024 {
            changes.observe(Ok(modified()));
        }
        assert_eq!(changes.pending_since, Some(first));
        assert_eq!(changes.ready_generation, 0);
        assert!(changes.last_event.unwrap() > first);
    }
    let changes = index.current_changes().unwrap();
    assert_eq!(changes.ready_generation, 1025);
    assert_eq!(changes.pending_since, None);
}

#[test]
fn unknown_rescan_and_overflow_bypass_debounce() {
    for event in [
        Event::new(EventKind::Any).add_path(PathBuf::from("vault/tasks/tasks.md")),
        Event::new(EventKind::Other),
        modified().set_flag(Flag::Rescan),
    ] {
        let mut changes = Changes::default();
        changes.observe(Ok(event));
        assert_eq!(changes.ready_generation, changes.generation);
        assert_eq!(changes.generation, 1);
        assert_eq!(changes.pending_since, None);
    }
    let mut changes = Changes {
        generation: u64::MAX,
        ..Changes::default()
    };
    changes.observe(Ok(modified()));
    assert!(changes.failed);
}

#[test]
fn read_snapshot_and_non_markdown_file_events_are_ignored() {
    for (kind, path) in [
        (
            EventKind::Access(AccessKind::Read),
            "vault/tasks/FOO-0001.md",
        ),
        (
            EventKind::Access(AccessKind::Open(AccessMode::Any)),
            "vault/tasks/FOO-0001.md",
        ),
        (
            EventKind::Access(AccessKind::Close(AccessMode::Read)),
            "vault/tasks/FOO-0001.md",
        ),
        (EventKind::Create(CreateKind::File), "vault/cache.db"),
        (EventKind::Remove(RemoveKind::File), "vault/service.log"),
        (
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            "vault/cache.db-wal",
        ),
        (
            EventKind::Access(AccessKind::Close(AccessMode::Write)),
            "vault/service.log",
        ),
        (
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            "vault/tasks/tasks.md",
        ),
        (
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            "vault/tasks/tasks.backup.md",
        ),
    ] {
        let mut changes = Changes::default();
        changes.observe(Ok(Event::new(kind).add_path(PathBuf::from(path))));
        assert_eq!(changes.generation, 0, "{kind:?}: {path}");
    }
}

#[test]
fn dotted_directory_events_and_ambiguous_metadata_remain_relevant() {
    for kind in [
        EventKind::Create(CreateKind::Folder),
        EventKind::Remove(RemoveKind::Folder),
        EventKind::Modify(ModifyKind::Name(RenameMode::From)),
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)),
    ] {
        let mut changes = Changes::default();
        changes.observe(Ok(
            Event::new(kind).add_path(PathBuf::from("vault/tasks.v2"))
        ));
        assert_eq!(changes.generation, 1, "{kind:?}");
    }
    let mut changes = Changes::default();
    changes.observe(Ok(modified().add_path(PathBuf::from("vault/cache.db"))));
    assert_eq!(changes.generation, 1);
    for kind in [
        EventKind::Create(CreateKind::Folder),
        EventKind::Remove(RemoveKind::Folder),
    ] {
        let mut changes = Changes::default();
        changes.observe(Ok(
            Event::new(kind).add_path(PathBuf::from("vault/tasks/tasks.md"))
        ));
        assert_eq!(
            changes.generation, 1,
            "directory with a snapshot filename: {kind:?}"
        );
    }
}

#[test]
fn rescan_invalidates_every_directory_and_releases_shared_roots() {
    let root = tempfile::tempdir().unwrap();
    let first = directory(root.path(), "first");
    let second = directory(root.path(), "second");
    let index = TaskIndex::new();
    let first_before = index.read(&first, || Ok(summaries(&first, 0))).unwrap();
    let second_before = index.read(&second, || Ok(summaries(&second, 0))).unwrap();
    index
        .changes
        .lock()
        .unwrap()
        .observe(Ok(modified().set_flag(Flag::Rescan)));

    let first_after = index.read(&first, || Ok(summaries(&first, 0))).unwrap();
    assert!(!Arc::ptr_eq(&first_before, &first_after));
    {
        let state = index.state.lock().unwrap();
        assert_eq!(state.entries.len(), 1);
        assert_eq!(watch_users(&state, &second), None);
        assert_eq!(watch_users(&state, root.path()), Some(1));
    }
    let second_after = index.read(&second, || Ok(summaries(&second, 0))).unwrap();
    assert!(!Arc::ptr_eq(&second_before, &second_after));
}
