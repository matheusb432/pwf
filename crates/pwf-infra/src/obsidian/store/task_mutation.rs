use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

use pwf_application::ports::task_vault::{
    ExpectedTaskRevision, IndexEntry, TaskMutationError, TaskPatch, TaskRevisionState, TaskWrite,
    TaskWriteSet,
};
use pwf_models::{project::Project, task::TaskId};

use super::{
    ObsidianStore, ObsidianStoreError,
    index_entry::{delete_index_entry_text, parse_index_lines, upsert_index_entry_text},
    task_record::patch_index_entry_text,
};
use crate::{
    file_transaction::{
        FileSnapshot, FileState, FileTransaction, FileTransactionError, MissingFileSnapshot,
        PresentFileSnapshot, snapshot,
    },
    obsidian::{
        MarkdownFile,
        identity::{
            new_project_index_content, parse_project_index_identity,
            validate_project_index_identity,
        },
        trash::task_file_trash_destination,
    },
};

#[derive(Clone)]
enum TaskBacking {
    Note(PresentFileSnapshot),
    Index,
}

struct IndexDocument {
    snapshot: FileSnapshot,
    source: String,
    change_queued: bool,
}

enum PendingChange {
    ReplaceNote {
        current: PresentFileSnapshot,
        contents: Box<[u8]>,
    },
    ReplaceIndex,
    RemoveNote {
        current: PresentFileSnapshot,
    },
    Rename {
        source: PresentFileSnapshot,
        destination: MissingFileSnapshot,
    },
}

impl ObsidianStore {
    pub(super) fn commit_task_writes_impl(
        &self,
        project: &Project,
        writes: TaskWriteSet,
    ) -> Result<(), TaskMutationError<ObsidianStoreError>> {
        let (expected, writes) = writes.into_parts();
        let mut mutation = TaskMutation::new(self, project)?;
        mutation.resolve_expectations(expected)?;
        mutation.apply_writes(writes)?;
        mutation.commit()
    }
}

struct TaskMutation<'a> {
    store: &'a ObsidianStore,
    project: &'a Project,
    task_paths: BTreeMap<TaskId, PathBuf>,
    index: Option<IndexDocument>,
    backings: BTreeMap<TaskId, TaskBacking>,
    expected_by_path: BTreeMap<PathBuf, ExpectedTaskRevision>,
    expected_by_id: BTreeMap<TaskId, ExpectedTaskRevision>,
    changes: Vec<PendingChange>,
}

impl<'a> TaskMutation<'a> {
    fn new(
        store: &'a ObsidianStore,
        project: &'a Project,
    ) -> Result<Self, TaskMutationError<ObsidianStoreError>> {
        let task_paths = store
            .task_files_for_project(project)
            .map_err(TaskMutationError::Store)?
            .into_iter()
            .map(|task| (task.id, task.path))
            .collect();
        Ok(Self {
            store,
            project,
            task_paths,
            index: None,
            backings: BTreeMap::new(),
            expected_by_path: BTreeMap::new(),
            expected_by_id: BTreeMap::new(),
            changes: Vec::new(),
        })
    }

    fn resolve_expectations(
        &mut self,
        expected: Vec<ExpectedTaskRevision>,
    ) -> Result<(), TaskMutationError<ObsidianStoreError>> {
        for expectation in expected {
            self.resolve_expectation(expectation)?;
        }
        Ok(())
    }

    fn resolve_expectation(
        &mut self,
        expectation: ExpectedTaskRevision,
    ) -> Result<(), TaskMutationError<ObsidianStoreError>> {
        let backing = resolve_backing(
            self.store,
            self.project,
            &self.task_paths,
            &mut self.index,
            &expectation,
        )?;
        let path = backing_path(&backing, self.index.as_ref(), &expectation.id)?;
        self.expected_by_path
            .entry(path)
            .or_insert_with(|| expectation.clone());
        self.expected_by_id
            .insert(expectation.id.clone(), expectation.clone());
        self.backings.insert(expectation.id, backing);
        Ok(())
    }

    fn apply_writes(
        &mut self,
        writes: Vec<TaskWrite>,
    ) -> Result<(), TaskMutationError<ObsidianStoreError>> {
        for write in writes {
            self.apply_write(write)?;
        }
        Ok(())
    }

    fn apply_write(
        &mut self,
        write: TaskWrite,
    ) -> Result<(), TaskMutationError<ObsidianStoreError>> {
        match write {
            TaskWrite::Patch { id, patch } => self.patch(&id, &patch),
            TaskWrite::DeleteNote { id, deletion } => self.delete_note(&id, &deletion),
            TaskWrite::UpsertIndex(entry) => self.upsert_index(&entry),
            TaskWrite::DeleteIndex(id) => self.delete_index(&id),
        }
    }

    fn patch(
        &mut self,
        id: &TaskId,
        patch: &TaskPatch,
    ) -> Result<(), TaskMutationError<ObsidianStoreError>> {
        match backing(&self.backings, id)?.clone() {
            TaskBacking::Note(current) => self.patch_note(current, patch),
            TaskBacking::Index => self.patch_index(id, patch),
        }
    }

    fn patch_note(
        &mut self,
        current: PresentFileSnapshot,
        patch: &TaskPatch,
    ) -> Result<(), TaskMutationError<ObsidianStoreError>> {
        let mut file = MarkdownFile::from_snapshot(current).map_err(markdown_store_error)?;
        ObsidianStore::apply_task_patch(&mut file, patch).map_err(TaskMutationError::Store)?;
        let (current, contents) = file.into_replacement().map_err(markdown_store_error)?;
        self.changes
            .push(PendingChange::ReplaceNote { current, contents });
        Ok(())
    }

    fn patch_index(
        &mut self,
        id: &TaskId,
        patch: &TaskPatch,
    ) -> Result<(), TaskMutationError<ObsidianStoreError>> {
        let document = index_document(self.store, self.project, &mut self.index)?;
        let line = parse_index_lines(document.snapshot.path(), &document.source)
            .map_err(TaskMutationError::Store)?
            .into_iter()
            .find(|line| line.id == *id)
            .ok_or_else(|| stale_missing(id, &self.expected_by_id))?;
        let updated =
            patch_index_entry_text(document.snapshot.path(), &document.source, &line, patch)
                .map_err(TaskMutationError::Store)?;
        update_index(document, updated, &mut self.changes);
        Ok(())
    }

    fn delete_note(
        &mut self,
        id: &TaskId,
        deletion: &pwf_wire::confirmation::TaskDeletion,
    ) -> Result<(), TaskMutationError<ObsidianStoreError>> {
        let configured =
            pwf_application::ports::task_vault::TaskVault::task_deletion(self.store, self.project)
                .map_err(TaskMutationError::Store)?;
        if &configured != deletion {
            return Err(TaskMutationError::Store(
                ObsidianStoreError::TaskDeletionChanged,
            ));
        }
        let source = note_backing(&self.backings, id)?;
        match deletion.trash_folder() {
            None => self
                .changes
                .push(PendingChange::RemoveNote { current: source }),
            Some(trash_folder) => {
                let destination_path = task_file_trash_destination(source.path(), &trash_folder)
                    .map_err(TaskMutationError::Store)?;
                let destination = missing_snapshot(&destination_path)?;
                self.changes.push(PendingChange::Rename {
                    source,
                    destination,
                });
            }
        }
        Ok(())
    }

    fn upsert_index(
        &mut self,
        entry: &IndexEntry,
    ) -> Result<(), TaskMutationError<ObsidianStoreError>> {
        let document = index_document(self.store, self.project, &mut self.index)?;
        let updated = upsert_index_entry_text(document.snapshot.path(), &document.source, entry)
            .map_err(TaskMutationError::Store)?;
        update_index(document, updated, &mut self.changes);
        Ok(())
    }

    fn delete_index(&mut self, id: &TaskId) -> Result<(), TaskMutationError<ObsidianStoreError>> {
        let document = index_document(self.store, self.project, &mut self.index)?;
        let updated = delete_index_entry_text(&document.source, id);
        update_index(document, updated, &mut self.changes);
        Ok(())
    }

    fn commit(self) -> Result<(), TaskMutationError<ObsidianStoreError>> {
        commit_file_transaction(
            &self.backings,
            self.index.as_ref(),
            self.changes,
            &self.expected_by_path,
        )
    }
}

fn backing_path(
    backing: &TaskBacking,
    index: Option<&IndexDocument>,
    id: &TaskId,
) -> Result<PathBuf, TaskMutationError<ObsidianStoreError>> {
    match backing {
        TaskBacking::Note(snapshot) => Ok(snapshot.path().to_path_buf()),
        TaskBacking::Index => index
            .map(|document| document.snapshot.path().to_path_buf())
            .ok_or_else(|| {
                TaskMutationError::Store(ObsidianStoreError::TaskNotFound { id: id.clone() })
            }),
    }
}

fn note_backing(
    backings: &BTreeMap<TaskId, TaskBacking>,
    id: &TaskId,
) -> Result<PresentFileSnapshot, TaskMutationError<ObsidianStoreError>> {
    match backing(backings, id)? {
        TaskBacking::Note(snapshot) => Ok(snapshot.clone()),
        TaskBacking::Index => Err(TaskMutationError::Store(ObsidianStoreError::TaskNotFound {
            id: id.clone(),
        })),
    }
}

fn missing_snapshot(
    path: &Path,
) -> Result<MissingFileSnapshot, TaskMutationError<ObsidianStoreError>> {
    match snapshot(path).map_err(file_store_error)? {
        FileSnapshot::Missing(snapshot) => Ok(snapshot),
        FileSnapshot::Present(_) => Err(TaskMutationError::Store(
            ObsidianStoreError::TaskTrashDestinationExists {
                path: path.to_path_buf(),
            },
        )),
    }
}

fn commit_file_transaction(
    backings: &BTreeMap<TaskId, TaskBacking>,
    index: Option<&IndexDocument>,
    changes: Vec<PendingChange>,
    expected_by_path: &BTreeMap<PathBuf, ExpectedTaskRevision>,
) -> Result<(), TaskMutationError<ObsidianStoreError>> {
    let mut transaction = FileTransaction::new();
    observe_backings(&mut transaction, backings, expected_by_path)?;
    observe_index(&mut transaction, index, expected_by_path)?;
    for change in changes {
        add_pending_change(&mut transaction, change, index, expected_by_path)?;
    }
    transaction
        .commit()
        .map_err(|error| map_file_error(error, expected_by_path))
}

fn observe_backings(
    transaction: &mut FileTransaction,
    backings: &BTreeMap<TaskId, TaskBacking>,
    expected_by_path: &BTreeMap<PathBuf, ExpectedTaskRevision>,
) -> Result<(), TaskMutationError<ObsidianStoreError>> {
    for backing in backings.values() {
        let TaskBacking::Note(snapshot) = backing else {
            continue;
        };
        transaction
            .expect(FileSnapshot::Present(snapshot.clone()))
            .map_err(|error| map_file_error(error, expected_by_path))?;
    }
    Ok(())
}

fn observe_index(
    transaction: &mut FileTransaction,
    index: Option<&IndexDocument>,
    expected_by_path: &BTreeMap<PathBuf, ExpectedTaskRevision>,
) -> Result<(), TaskMutationError<ObsidianStoreError>> {
    let Some(document) = index else {
        return Ok(());
    };
    transaction
        .expect(document.snapshot.clone())
        .map_err(|error| map_file_error(error, expected_by_path))
}

fn add_pending_change(
    transaction: &mut FileTransaction,
    change: PendingChange,
    index: Option<&IndexDocument>,
    expected_by_path: &BTreeMap<PathBuf, ExpectedTaskRevision>,
) -> Result<(), TaskMutationError<ObsidianStoreError>> {
    match change {
        PendingChange::RemoveNote { current } => transaction.remove(current),
        PendingChange::ReplaceNote { current, contents } => {
            transaction.replace(FileSnapshot::Present(current), contents)
        }
        PendingChange::ReplaceIndex => {
            let document = index.ok_or_else(missing_index_document)?;
            transaction.replace(
                document.snapshot.clone(),
                document.source.clone().into_bytes().into_boxed_slice(),
            )
        }
        PendingChange::Rename {
            source,
            destination,
        } => transaction.rename(source, destination),
    }
    .map_err(|error| map_file_error(error, expected_by_path))
}

fn missing_index_document() -> TaskMutationError<ObsidianStoreError> {
    TaskMutationError::Store(ObsidianStoreError::TaskMutationFilesystem {
        source: io::Error::other("index replacement has no observed index document"),
    })
}

fn resolve_backing(
    store: &ObsidianStore,
    project: &Project,
    task_paths: &BTreeMap<TaskId, PathBuf>,
    index: &mut Option<IndexDocument>,
    expected: &ExpectedTaskRevision,
) -> Result<TaskBacking, TaskMutationError<ObsidianStoreError>> {
    if let Some(path) = task_paths.get(&expected.id) {
        let current = snapshot(path).map_err(file_store_error)?;
        let FileSnapshot::Present(current) = current else {
            return Err(stale(expected, TaskRevisionState::Missing));
        };
        if current.revision() != &expected.revision {
            return Err(stale(
                expected,
                TaskRevisionState::Present(current.revision().clone()),
            ));
        }
        return Ok(TaskBacking::Note(current));
    }

    let document = index_document(store, project, index)?;
    let FileSnapshot::Present(current) = &document.snapshot else {
        return Err(stale(expected, TaskRevisionState::Missing));
    };
    let exists = parse_index_lines(current.path(), &document.source)
        .map_err(TaskMutationError::Store)?
        .into_iter()
        .any(|line| line.id == expected.id);
    if !exists {
        return Err(stale(expected, TaskRevisionState::Missing));
    }
    if current.revision() != &expected.revision {
        return Err(stale(
            expected,
            TaskRevisionState::Present(current.revision().clone()),
        ));
    }
    Ok(TaskBacking::Index)
}

fn index_document<'a>(
    store: &ObsidianStore,
    project: &Project,
    index: &'a mut Option<IndexDocument>,
) -> Result<&'a mut IndexDocument, TaskMutationError<ObsidianStoreError>> {
    if index.is_none() {
        *index = Some(read_index_document(store, project)?);
    }
    index.as_mut().ok_or_else(|| {
        TaskMutationError::Store(ObsidianStoreError::TaskMutationFilesystem {
            source: io::Error::other("index document was not initialized"),
        })
    })
}

fn read_index_document(
    store: &ObsidianStore,
    project: &Project,
) -> Result<IndexDocument, TaskMutationError<ObsidianStoreError>> {
    let path = store
        .project_index_path(project)
        .map_err(TaskMutationError::Store)?;
    let observed = snapshot(&path).map_err(file_store_error)?;
    let source = index_source(&path, &observed, project)?;
    Ok(IndexDocument {
        snapshot: observed,
        source,
        change_queued: false,
    })
}

fn index_source(
    path: &Path,
    observed: &FileSnapshot,
    project: &Project,
) -> Result<String, TaskMutationError<ObsidianStoreError>> {
    match observed {
        FileSnapshot::Present(current) => present_index_source(path, current, project),
        FileSnapshot::Missing(_) => Ok(new_project_index_content(
            &ObsidianStore::project_identity(project),
        )),
    }
}

fn present_index_source(
    path: &Path,
    current: &PresentFileSnapshot,
    project: &Project,
) -> Result<String, TaskMutationError<ObsidianStoreError>> {
    let source = String::from_utf8(current.bytes().to_vec()).map_err(invalid_index_utf8)?;
    let file = MarkdownFile::from_source(path.to_path_buf(), source.clone());
    let actual = parse_project_index_identity(&file).map_err(TaskMutationError::Store)?;
    validate_project_index_identity(path, &actual, &ObsidianStore::project_identity(project))
        .map_err(TaskMutationError::Store)?;
    Ok(source)
}

fn invalid_index_utf8(source: std::string::FromUtf8Error) -> TaskMutationError<ObsidianStoreError> {
    TaskMutationError::Store(ObsidianStoreError::ReadIndex {
        source: io::Error::new(io::ErrorKind::InvalidData, source),
    })
}

fn update_index(document: &mut IndexDocument, updated: String, changes: &mut Vec<PendingChange>) {
    if updated == document.source {
        return;
    }
    document.source = updated;
    if !document.change_queued {
        document.change_queued = true;
        changes.push(PendingChange::ReplaceIndex);
    }
}

fn backing<'a>(
    backings: &'a BTreeMap<TaskId, TaskBacking>,
    id: &TaskId,
) -> Result<&'a TaskBacking, TaskMutationError<ObsidianStoreError>> {
    backings.get(id).ok_or_else(|| {
        TaskMutationError::Store(ObsidianStoreError::TaskNotFound { id: id.clone() })
    })
}

fn stale(
    expected: &ExpectedTaskRevision,
    current: TaskRevisionState,
) -> TaskMutationError<ObsidianStoreError> {
    TaskMutationError::StaleTask {
        id: expected.id.clone(),
        expected: expected.revision.clone(),
        current,
    }
}

fn stale_missing(
    id: &TaskId,
    expected_by_id: &BTreeMap<TaskId, ExpectedTaskRevision>,
) -> TaskMutationError<ObsidianStoreError> {
    match expected_by_id.get(id) {
        Some(expected) => stale(expected, TaskRevisionState::Missing),
        None => TaskMutationError::Store(ObsidianStoreError::TaskNotFound { id: id.clone() }),
    }
}

fn map_file_error(
    error: FileTransactionError,
    expected_by_path: &BTreeMap<PathBuf, ExpectedTaskRevision>,
) -> TaskMutationError<ObsidianStoreError> {
    match error {
        FileTransactionError::Changed { path, current, .. } => {
            if let Some(expected) = expected_by_path.get(&path) {
                return stale(expected, revision_state(current));
            }
            TaskMutationError::SourceChanged
        }
        error => file_store_error(error),
    }
}

fn revision_state(state: FileState) -> TaskRevisionState {
    match state {
        FileState::Present(revision) => TaskRevisionState::Present(revision),
        FileState::Missing => TaskRevisionState::Missing,
    }
}

fn file_store_error(source: FileTransactionError) -> TaskMutationError<ObsidianStoreError> {
    TaskMutationError::Store(ObsidianStoreError::TaskMutationFilesystem {
        source: io::Error::other(source),
    })
}

fn markdown_store_error(
    source: crate::obsidian::MarkdownFileError,
) -> TaskMutationError<ObsidianStoreError> {
    TaskMutationError::Store(ObsidianStoreError::WriteTaskFile {
        source: source.into_io_error(),
    })
}
