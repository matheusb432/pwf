use std::{
    collections::VecDeque,
    mem::size_of,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _,
    event::{AccessKind, AccessMode, CreateKind, ModifyKind, RemoveKind},
};

use super::{
    ObsidianStoreError, identity::TaskFile, project_snapshot_backup_path, project_snapshot_path,
};

const INDEX_BYTES_MAX: usize = 1024 * 1024;
const DIRECTORY_COUNT_MAX: usize = 16;
const WATCH_COUNT_MAX: usize = DIRECTORY_COUNT_MAX * 2;
const DEBOUNCE: Duration = Duration::from_millis(50);
const AGE_MAX: Duration = Duration::from_secs(60);

pub(super) struct TaskIndex {
    state: Mutex<IndexState>,
    changes: Arc<Mutex<Changes>>,
}

struct IndexState {
    entries: VecDeque<Entry>,
    watches: Vec<(PathBuf, usize)>,
    watcher: Option<RecommendedWatcher>,
    bytes: usize,
    generation: u64,
}

struct Entry {
    directory: PathBuf,
    notes: Arc<[TaskFile]>,
    scanned_at: Instant,
    bytes: usize,
}

#[derive(Clone, Copy, Default)]
struct Changes {
    generation: u64,
    ready_generation: u64,
    pending_since: Option<Instant>,
    last_event: Option<Instant>,
    failed: bool,
}

impl TaskIndex {
    pub(super) fn new() -> Self {
        let changes = Arc::new(Mutex::new(Changes::default()));
        let callback_changes = Arc::clone(&changes);
        let watcher = notify::recommended_watcher(move |event| {
            if let Ok(mut changes) = callback_changes.lock() {
                changes.observe(event);
            }
        })
        .ok();
        Self {
            state: Mutex::new(IndexState {
                entries: VecDeque::with_capacity(DIRECTORY_COUNT_MAX),
                watches: Vec::with_capacity(WATCH_COUNT_MAX),
                watcher,
                bytes: 0,
                generation: 0,
            }),
            changes,
        }
    }

    /// Reuses metadata for at most 50 ms after the first external event in a batch.
    /// Every entry is rescanned on its first read after 60 seconds, even without events.
    /// The caller supplies a complete directory scan; returned arcs can outlive eviction.
    pub(super) fn read(
        &self,
        directory: &Path,
        scan: impl FnOnce() -> Result<Vec<TaskFile>, ObsidianStoreError>,
    ) -> Result<Arc<[TaskFile]>, ObsidianStoreError> {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                poisoned.into_inner().disable();
                return scan().map(Arc::from);
            }
        };
        let Some(changes) = self.current_changes() else {
            state.disable();
            return scan().map(Arc::from);
        };
        if state.generation != changes.ready_generation {
            // Release shared roots too: a replaced directory may have lost its OS watch.
            while !state.entries.is_empty() {
                state.remove(0);
            }
            state.generation = changes.ready_generation;
        }
        if let Some(position) = state
            .entries
            .iter()
            .position(|entry| entry.directory == directory)
        {
            let entry = &state.entries[position];
            if entry.scanned_at.elapsed() < AGE_MAX {
                return Ok(Arc::clone(&entry.notes));
            }
            state.remove(position);
        }
        let Some(parent) = directory.parent() else {
            return scan().map(Arc::from);
        };
        if state.entries.len() == DIRECTORY_COUNT_MAX {
            state.remove(0);
        }
        if !state.watch(parent) || !state.watch(directory) {
            state.disable();
            return scan().map(Arc::from);
        }
        // Never hold Changes while watching, unwatching, dropping the watcher, or scanning.
        let Some(before) = self.current_changes() else {
            state.disable();
            return scan().map(Arc::from);
        };
        let scanned_at = Instant::now();
        let result = scan().map(Arc::<[TaskFile]>::from);
        let Some(after) = self.current_changes() else {
            state.disable();
            return result;
        };
        if before.generation == after.generation
            && let Ok(notes) = &result
            && state.retain(directory, parent, notes, scanned_at)
        {
            return result;
        }
        state.unwatch(directory);
        state.unwatch(parent);
        result
    }

    /// Own writes remove their directory immediately, without the external debounce.
    pub(super) fn invalidate(&self, directory: &Path) {
        match self.state.lock() {
            Ok(mut state) => {
                if let Some(position) = state
                    .entries
                    .iter()
                    .position(|entry| entry.directory == directory)
                {
                    state.remove(position);
                }
            }
            Err(poisoned) => poisoned.into_inner().disable(),
        }
    }

    fn current_changes(&self) -> Option<Changes> {
        let mut changes = self.changes.lock().ok()?;
        if changes.failed {
            return None;
        }
        if changes
            .pending_since
            .is_some_and(|first| first.elapsed() >= DEBOUNCE)
        {
            changes.ready_generation = changes.generation;
            changes.pending_since = None;
        }
        Some(*changes)
    }
}

impl IndexState {
    fn retain(
        &mut self,
        directory: &Path,
        parent: &Path,
        notes: &Arc<[TaskFile]>,
        scanned_at: Instant,
    ) -> bool {
        let bytes = entry_bytes(directory, notes);
        let watches_bytes = directory
            .as_os_str()
            .len()
            .saturating_add(parent.as_os_str().len());
        if self
            .overhead()
            .saturating_add(watches_bytes)
            .saturating_add(bytes)
            > INDEX_BYTES_MAX
        {
            return false;
        }
        while self.accounted_bytes().saturating_add(bytes) > INDEX_BYTES_MAX
            && !self.entries.is_empty()
        {
            self.remove(0);
        }
        if self.watcher.is_none() {
            return false;
        }
        self.bytes += bytes;
        self.entries.push_back(Entry {
            directory: directory.to_path_buf(),
            notes: Arc::clone(notes),
            scanned_at,
            bytes,
        });
        true
    }

    fn overhead(&self) -> usize {
        // Charge both Arc headers and every reserved entry/watch slot, including unused slots.
        size_of::<TaskIndex>()
            + size_of::<Mutex<Changes>>()
            + 2 * size_of::<[usize; 2]>()
            + self.entries.capacity() * size_of::<Entry>()
            + self.watches.capacity() * size_of::<(PathBuf, usize)>()
    }

    fn accounted_bytes(&self) -> usize {
        self.overhead().saturating_add(self.bytes)
    }

    fn disable(&mut self) {
        self.watcher = None;
        self.entries.clear();
        self.watches.clear();
        self.bytes = 0;
    }

    fn watch(&mut self, path: &Path) -> bool {
        if self.watcher.is_none() {
            return false;
        }
        if let Some((_, users)) = self.watches.iter_mut().find(|(watched, _)| watched == path) {
            *users += 1;
            return true;
        }
        let bytes = path.as_os_str().len();
        while self.accounted_bytes().saturating_add(bytes) > INDEX_BYTES_MAX
            && !self.entries.is_empty()
        {
            self.remove(0);
        }
        if self.watches.len() == WATCH_COUNT_MAX
            || self.accounted_bytes().saturating_add(bytes) > INDEX_BYTES_MAX
            || self
                .watcher
                .as_mut()
                .is_none_or(|watcher| watcher.watch(path, RecursiveMode::NonRecursive).is_err())
        {
            return false;
        }
        self.watches.push((path.to_path_buf(), 1));
        self.bytes += bytes;
        true
    }

    fn unwatch(&mut self, path: &Path) {
        let Some(position) = self.watches.iter().position(|(watched, _)| watched == path) else {
            return;
        };
        self.watches[position].1 -= 1;
        if self.watches[position].1 > 0 {
            return;
        }
        if !self
            .watcher
            .as_mut()
            .is_some_and(|watcher| match watcher.unwatch(path) {
                Ok(()) => true,
                Err(error) => matches!(error.kind, notify::ErrorKind::WatchNotFound),
            })
        {
            self.disable();
            return;
        }
        let (path, _) = self.watches.swap_remove(position);
        self.bytes -= path.as_os_str().len();
    }

    fn remove(&mut self, position: usize) {
        if let Some(entry) = self.entries.remove(position) {
            self.bytes -= entry.bytes;
            self.unwatch(&entry.directory);
            if let Some(parent) = entry.directory.parent() {
                self.unwatch(parent);
            }
        }
    }
}

impl Changes {
    fn observe(&mut self, result: notify::Result<Event>) {
        let Ok(event) = result else {
            self.failed = true;
            return;
        };
        let immediate = event.need_rescan()
            || event.paths.is_empty()
            || matches!(event.kind, EventKind::Any | EventKind::Other);
        // Rename and metadata events can describe directories, even with a file extension.
        let files_only = matches!(
            event.kind,
            EventKind::Create(CreateKind::File)
                | EventKind::Remove(RemoveKind::File)
                | EventKind::Modify(ModifyKind::Data(_))
                | EventKind::Access(AccessKind::Close(AccessMode::Write))
        );
        let directories_only = matches!(
            event.kind,
            EventKind::Create(CreateKind::Folder) | EventKind::Remove(RemoveKind::Folder)
        );
        if !immediate
            && (matches!(
                event.kind,
                EventKind::Access(
                    AccessKind::Read | AccessKind::Open(_) | AccessKind::Close(AccessMode::Read)
                )
            ) || event.paths.iter().all(|path| {
                (!directories_only && generated_snapshot(path))
                    || (files_only && path.extension().is_some_and(|extension| extension != "md"))
            }))
        {
            return;
        }
        let Some(generation) = self.generation.checked_add(1) else {
            self.failed = true;
            return;
        };
        self.generation = generation;
        self.last_event = Some(Instant::now());
        if immediate {
            self.ready_generation = generation;
            self.pending_since = None;
        } else {
            self.pending_since = self.pending_since.or(self.last_event);
        }
    }
}

fn entry_bytes(directory: &Path, notes: &[TaskFile]) -> usize {
    notes.iter().fold(
        directory
            .as_os_str()
            .len()
            .saturating_add(size_of::<[usize; 2]>()),
        |bytes, note| {
            [
                size_of::<TaskFile>(),
                note.path.as_os_str().len(),
                note.id.as_ref().len(),
                note.id.project_id().as_ref().len(),
                note.summary.as_ref().map_or(0, |summary| {
                    [
                        summary.id.as_ref().len(),
                        summary.id.project_id().as_ref().len(),
                        summary.title.capacity(),
                        summary.effort.as_ref().map_or(0, String::capacity),
                        summary.priority.as_ref().map_or(0, String::capacity),
                        summary.tags.as_ref().map_or(0, |tags| tags.as_ref().len()),
                    ]
                    .into_iter()
                    .fold(0, usize::saturating_add)
                }),
            ]
            .into_iter()
            .fold(bytes, usize::saturating_add)
        },
    )
}

fn generated_snapshot(path: &Path) -> bool {
    path.parent().is_some_and(|parent| {
        project_snapshot_path(parent).as_deref() == Some(path)
            || project_snapshot_backup_path(parent).as_deref() == Some(path)
    })
}

#[cfg(test)]
mod tests;
