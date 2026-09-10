use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

use pwf_application::ports::task_vault::{
    ExpectedTaskRevision, TaskMutationError, TaskRevisionState, TaskVault, TaskWrite, TaskWriteSet,
};
use pwf_models::{project::Project, task::TaskId};

use super::{ObsidianStore, ObsidianStoreError};
use crate::{
    file_transaction::{
        FileSnapshot, FileState, FileTransaction, FileTransactionError, MissingFileSnapshot,
        PresentFileSnapshot, snapshot,
    },
    obsidian::{MarkdownFile, trash::task_file_trash_destination},
};

impl ObsidianStore {
    pub(super) fn commit_task_writes_impl(
        &self,
        project: &Project,
        writes: TaskWriteSet,
    ) -> Result<(), TaskMutationError<ObsidianStoreError>> {
        let task_paths: BTreeMap<_, _> = self
            .task_files_for_project(project)
            .map_err(TaskMutationError::Store)?
            .into_iter()
            .map(|task| (task.id, task.path))
            .collect();
        let (expected, writes) = writes.into_parts();
        let mut notes = BTreeMap::new();
        let mut expected_by_path = BTreeMap::new();
        let mut transaction = FileTransaction::new();
        for expectation in expected {
            let note = resolve_note(&task_paths, &expectation)?;
            expected_by_path.insert(note.path().to_path_buf(), expectation.clone());
            transaction
                .expect(FileSnapshot::Present(note.clone()))
                .map_err(|error| map_file_error(error, &expected_by_path))?;
            notes.insert(expectation.id, note);
        }
        for write in writes {
            self.prepare_task_write(project, &notes, &mut transaction, write)?;
        }
        transaction
            .commit()
            .map_err(|error| map_file_error(error, &expected_by_path))
    }

    fn prepare_task_write(
        &self,
        project: &Project,
        notes: &BTreeMap<TaskId, PresentFileSnapshot>,
        transaction: &mut FileTransaction,
        write: TaskWrite,
    ) -> Result<(), TaskMutationError<ObsidianStoreError>> {
        match write {
            TaskWrite::Patch { id, patch } => {
                let mut file = MarkdownFile::from_snapshot(note_snapshot(notes, &id)?)
                    .map_err(markdown_store_error)?;
                Self::apply_task_patch(&mut file, &patch).map_err(TaskMutationError::Store)?;
                let (current, contents) = file.into_replacement().map_err(markdown_store_error)?;
                transaction
                    .replace(FileSnapshot::Present(current), contents)
                    .map_err(file_store_error)
            }
            TaskWrite::DeleteNote { id, deletion } => {
                let configured = self
                    .task_deletion(project)
                    .map_err(TaskMutationError::Store)?;
                if configured != deletion {
                    return Err(TaskMutationError::Store(
                        ObsidianStoreError::TaskDeletionChanged,
                    ));
                }
                let source = note_snapshot(notes, &id)?;
                match deletion.trash_folder() {
                    None => transaction.remove(source).map_err(file_store_error),
                    Some(trash_folder) => {
                        let destination = task_file_trash_destination(source.path(), &trash_folder)
                            .map_err(TaskMutationError::Store)?;
                        transaction
                            .rename(source, missing_snapshot(&destination)?)
                            .map_err(file_store_error)
                    }
                }
            }
        }
    }
}

fn resolve_note(
    task_paths: &BTreeMap<TaskId, PathBuf>,
    expected: &ExpectedTaskRevision,
) -> Result<PresentFileSnapshot, TaskMutationError<ObsidianStoreError>> {
    let Some(path) = task_paths.get(&expected.id) else {
        return Err(stale(expected, TaskRevisionState::Missing));
    };
    let FileSnapshot::Present(current) = snapshot(path).map_err(file_store_error)? else {
        return Err(stale(expected, TaskRevisionState::Missing));
    };
    if current.revision() != &expected.revision {
        return Err(stale(
            expected,
            TaskRevisionState::Present(current.revision().clone()),
        ));
    }
    Ok(current)
}

fn note_snapshot(
    notes: &BTreeMap<TaskId, PresentFileSnapshot>,
    id: &TaskId,
) -> Result<PresentFileSnapshot, TaskMutationError<ObsidianStoreError>> {
    notes.get(id).cloned().ok_or_else(|| {
        TaskMutationError::Store(ObsidianStoreError::TaskNotFound { id: id.clone() })
    })
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

fn map_file_error(
    error: FileTransactionError,
    expected_by_path: &BTreeMap<PathBuf, ExpectedTaskRevision>,
) -> TaskMutationError<ObsidianStoreError> {
    match error {
        FileTransactionError::Changed { path, current, .. } => {
            if let Some(expected) = expected_by_path.get(&path) {
                let current = match current {
                    FileState::Present(revision) => TaskRevisionState::Present(revision),
                    FileState::Missing => TaskRevisionState::Missing,
                };
                return stale(expected, current);
            }
            TaskMutationError::SourceChanged
        }
        error => file_store_error(error),
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
