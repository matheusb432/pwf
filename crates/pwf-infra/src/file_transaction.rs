use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, Permissions},
    io::{self, Read as _, Write as _},
    path::{Path, PathBuf},
};

use atomic_write_file::AtomicWriteFile;
use pwf_models::revision::ContentRevision;

#[derive(Debug, Clone)]
pub(crate) enum FileSnapshot {
    Present(PresentFileSnapshot),
    Missing(MissingFileSnapshot),
}

impl FileSnapshot {
    pub(crate) fn into_present(self) -> Option<PresentFileSnapshot> {
        match self {
            Self::Present(snapshot) => Some(snapshot),
            Self::Missing(_) => None,
        }
    }

    #[cfg(test)]
    fn into_missing(self) -> Option<MissingFileSnapshot> {
        match self {
            Self::Missing(snapshot) => Some(snapshot),
            Self::Present(_) => None,
        }
    }

    pub(crate) fn path(&self) -> &Path {
        match self {
            Self::Present(snapshot) => snapshot.path(),
            Self::Missing(snapshot) => snapshot.path(),
        }
    }

    fn state(&self) -> FileState {
        match self {
            Self::Present(snapshot) => FileState::Present(snapshot.revision.clone()),
            Self::Missing(_) => FileState::Missing,
        }
    }

    fn has_same_contents(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Present(left), Self::Present(right)) => left.bytes == right.bytes,
            (Self::Missing(_), Self::Missing(_)) => true,
            (Self::Present(_), Self::Missing(_)) | (Self::Missing(_), Self::Present(_)) => false,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PresentFileSnapshot {
    path: PathBuf,
    bytes: Box<[u8]>,
    permissions: Permissions,
    revision: ContentRevision,
}

impl PresentFileSnapshot {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn revision(&self) -> &ContentRevision {
        &self.revision
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MissingFileSnapshot {
    path: PathBuf,
}

impl MissingFileSnapshot {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileState {
    Present(ContentRevision),
    Missing,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum FileTransactionError {
    #[error("failed to read file snapshot at {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("conflicting snapshots were supplied for {path}")]
    ConflictingObservation { path: PathBuf },
    #[error("more than one mutation targets {path}")]
    ConflictingIntent { path: PathBuf },
    #[error("file changed since it was read: {path}")]
    Changed {
        path: PathBuf,
        expected: FileState,
        current: FileState,
    },
    #[error("failed to create an atomic replacement for {path}")]
    OpenReplacement {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to write an atomic replacement for {path}")]
    WriteReplacement {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to synchronize an atomic replacement for {path}")]
    SyncReplacement {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to publish an atomic replacement for {path}")]
    CommitReplacement {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to remove {path}")]
    Remove {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("destination directory does not exist or is not a directory: {path}")]
    DestinationDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to rename {source_path} to {destination_path}")]
    Rename {
        source_path: PathBuf,
        destination_path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub(crate) fn snapshot(path: &Path) -> Result<FileSnapshot, FileTransactionError> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(FileSnapshot::Missing(MissingFileSnapshot {
                path: path.to_path_buf(),
            }));
        }
        Err(source) => {
            return Err(FileTransactionError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| FileTransactionError::Read {
            path: path.to_path_buf(),
            source,
        })?;
    let permissions = file
        .metadata()
        .map_err(|source| FileTransactionError::Read {
            path: path.to_path_buf(),
            source,
        })?
        .permissions();
    let revision = content_revision(&bytes);
    Ok(FileSnapshot::Present(PresentFileSnapshot {
        path: path.to_path_buf(),
        bytes: bytes.into_boxed_slice(),
        permissions,
        revision,
    }))
}

#[allow(clippy::expect_used)]
pub(crate) fn content_revision(bytes: &[u8]) -> ContentRevision {
    // BLAKE3's fixed lowercase hexadecimal encoding satisfies the validated newtype by contract.
    ContentRevision::try_new(blake3::hash(bytes).to_hex().to_string())
        .expect("BLAKE3 hexadecimal output satisfies ContentRevision")
}

#[derive(Debug)]
enum FileChange {
    Replace {
        path: PathBuf,
        contents: Box<[u8]>,
        permissions: Option<Permissions>,
    },
    Remove {
        path: PathBuf,
    },
    Rename {
        source_path: PathBuf,
        destination_path: PathBuf,
    },
}

pub(crate) struct FileTransaction {
    observations: BTreeMap<PathBuf, FileSnapshot>,
    mutation_paths: BTreeSet<PathBuf>,
    changes: Vec<FileChange>,
}

impl FileTransaction {
    pub(crate) fn new() -> Self {
        Self {
            observations: BTreeMap::new(),
            mutation_paths: BTreeSet::new(),
            changes: Vec::new(),
        }
    }

    pub(crate) fn expect(&mut self, observed: FileSnapshot) -> Result<(), FileTransactionError> {
        self.add_observation(observed).map(|_| ())
    }

    pub(crate) fn replace(
        &mut self,
        current: FileSnapshot,
        contents: Box<[u8]>,
    ) -> Result<(), FileTransactionError> {
        let permissions = match &current {
            FileSnapshot::Present(snapshot) => Some(snapshot.permissions.clone()),
            FileSnapshot::Missing(_) => None,
        };
        let path = self.add_observation(current)?;
        self.reserve_mutation_paths(std::slice::from_ref(&path))?;
        self.changes.push(FileChange::Replace {
            path,
            contents,
            permissions,
        });
        Ok(())
    }

    pub(crate) fn remove(
        &mut self,
        current: PresentFileSnapshot,
    ) -> Result<(), FileTransactionError> {
        let path = self.add_observation(FileSnapshot::Present(current))?;
        self.reserve_mutation_paths(std::slice::from_ref(&path))?;
        self.changes.push(FileChange::Remove { path });
        Ok(())
    }

    pub(crate) fn rename(
        &mut self,
        source: PresentFileSnapshot,
        destination: MissingFileSnapshot,
    ) -> Result<(), FileTransactionError> {
        let source_path = self.add_observation(FileSnapshot::Present(source))?;
        let destination_path = self.add_observation(FileSnapshot::Missing(destination))?;
        self.reserve_mutation_paths(&[source_path.clone(), destination_path.clone()])?;
        self.changes.push(FileChange::Rename {
            source_path,
            destination_path,
        });
        Ok(())
    }

    pub(crate) fn commit(self) -> Result<(), FileTransactionError> {
        let prepared = prepare_changes(self.changes)?;
        validate_observations(&self.observations)?;
        for change in &prepared {
            if let PreparedFileChange::Rename {
                destination_path, ..
            } = change
            {
                validate_destination_parent(destination_path)?;
            }
        }
        for change in prepared {
            change.commit()?;
        }
        Ok(())
    }

    fn add_observation(&mut self, observed: FileSnapshot) -> Result<PathBuf, FileTransactionError> {
        let path = observed.path().to_path_buf();
        if let Some(existing) = self.observations.get(&path) {
            return validate_existing_observation(existing, &observed, path);
        }
        self.observations.insert(path.clone(), observed);
        Ok(path)
    }

    fn reserve_mutation_paths(&mut self, paths: &[PathBuf]) -> Result<(), FileTransactionError> {
        if let Some(path) = paths
            .iter()
            .find(|path| self.mutation_paths.contains(path.as_path()))
        {
            return Err(FileTransactionError::ConflictingIntent { path: path.clone() });
        }
        self.mutation_paths.extend(paths.iter().cloned());
        Ok(())
    }
}

fn validate_existing_observation(
    existing: &FileSnapshot,
    observed: &FileSnapshot,
    path: PathBuf,
) -> Result<PathBuf, FileTransactionError> {
    if existing.has_same_contents(observed) {
        return Ok(path);
    }
    Err(FileTransactionError::ConflictingObservation { path })
}

enum PreparedFileChange {
    Replace {
        path: PathBuf,
        file: AtomicWriteFile,
    },
    Remove {
        path: PathBuf,
    },
    Rename {
        source_path: PathBuf,
        destination_path: PathBuf,
    },
}

impl PreparedFileChange {
    fn commit(self) -> Result<(), FileTransactionError> {
        match self {
            Self::Replace { path, file } => file
                .commit()
                .map_err(|source| FileTransactionError::CommitReplacement { path, source }),
            Self::Remove { path } => fs::remove_file(&path)
                .map_err(|source| FileTransactionError::Remove { path, source }),
            Self::Rename {
                source_path,
                destination_path,
            } => commit_rename(source_path, destination_path),
        }
    }
}

fn commit_rename(
    source_path: PathBuf,
    destination_path: PathBuf,
) -> Result<(), FileTransactionError> {
    fs::rename(&source_path, &destination_path).map_err(|source| FileTransactionError::Rename {
        source_path,
        destination_path,
        source,
    })
}

fn validate_destination_parent(destination_path: &Path) -> Result<(), FileTransactionError> {
    let Some(parent) = destination_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };
    match fs::metadata(parent) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        result => Err(FileTransactionError::DestinationDirectory {
            path: parent.to_path_buf(),
            source: result.err().unwrap_or_else(|| {
                io::Error::new(io::ErrorKind::NotADirectory, "expected directory")
            }),
        }),
    }
}

fn prepare_changes(
    changes: Vec<FileChange>,
) -> Result<Vec<PreparedFileChange>, FileTransactionError> {
    changes.into_iter().map(prepare_change).collect()
}

fn prepare_change(change: FileChange) -> Result<PreparedFileChange, FileTransactionError> {
    match change {
        FileChange::Replace {
            path,
            contents,
            permissions,
        } => {
            let mut file = AtomicWriteFile::open(&path).map_err(|source| {
                FileTransactionError::OpenReplacement {
                    path: path.clone(),
                    source,
                }
            })?;
            if let Some(permissions) = permissions {
                file.as_file()
                    .set_permissions(permissions)
                    .map_err(|source| FileTransactionError::WriteReplacement {
                        path: path.clone(),
                        source,
                    })?;
            }
            file.write_all(&contents)
                .map_err(|source| FileTransactionError::WriteReplacement {
                    path: path.clone(),
                    source,
                })?;
            file.sync_all()
                .map_err(|source| FileTransactionError::SyncReplacement {
                    path: path.clone(),
                    source,
                })?;
            Ok(PreparedFileChange::Replace { path, file })
        }
        FileChange::Remove { path } => Ok(PreparedFileChange::Remove { path }),
        FileChange::Rename {
            source_path,
            destination_path,
        } => Ok(PreparedFileChange::Rename {
            source_path,
            destination_path,
        }),
    }
}

fn validate_observations(
    observations: &BTreeMap<PathBuf, FileSnapshot>,
) -> Result<(), FileTransactionError> {
    for (path, expected) in observations {
        let current = current_state(path)?;
        let unchanged = match (expected, &current) {
            (FileSnapshot::Present(expected), CurrentFileState::Present(bytes)) => {
                expected.bytes.as_ref() == bytes.as_ref()
            }
            (FileSnapshot::Missing(_), CurrentFileState::Missing) => true,
            (FileSnapshot::Present(_), CurrentFileState::Missing)
            | (FileSnapshot::Missing(_), CurrentFileState::Present(_)) => false,
        };
        if !unchanged {
            return Err(FileTransactionError::Changed {
                path: path.clone(),
                expected: expected.state(),
                current: current.revision(),
            });
        }
    }
    Ok(())
}

enum CurrentFileState {
    Present(Box<[u8]>),
    Missing,
}

impl CurrentFileState {
    fn revision(&self) -> FileState {
        match self {
            Self::Present(bytes) => FileState::Present(content_revision(bytes)),
            Self::Missing => FileState::Missing,
        }
    }
}

fn current_state(path: &Path) -> Result<CurrentFileState, FileTransactionError> {
    match fs::read(path) {
        Ok(bytes) => Ok(CurrentFileState::Present(bytes.into_boxed_slice())),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(CurrentFileState::Missing),
        Err(source) => Err(FileTransactionError::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{FileSnapshot, FileTransaction, FileTransactionError, snapshot};

    fn present(snapshot: FileSnapshot) -> super::PresentFileSnapshot {
        snapshot.into_present().unwrap()
    }

    fn missing(snapshot: FileSnapshot) -> super::MissingFileSnapshot {
        snapshot.into_missing().unwrap()
    }

    #[test]
    fn snapshot_revision_uses_every_exact_file_byte() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("document.md");
        let bytes = b"\xef\xbb\xbf---\r\ntitle: exact\r\n---\r\n";
        fs::write(&path, bytes).unwrap();

        let observed = present(snapshot(&path).unwrap());

        assert_eq!(observed.revision(), &super::content_revision(bytes));
        assert_ne!(
            observed.revision(),
            &super::content_revision(b"---\ntitle: exact\n---\n")
        );
    }

    #[test]
    fn replacement_rejects_a_changed_source_before_publication() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("document.md");
        fs::write(&path, "original\n").unwrap();
        let observed = snapshot(&path).unwrap();
        let mut transaction = FileTransaction::new();
        transaction
            .replace(observed, Box::from(&b"replacement\n"[..]))
            .unwrap();

        fs::write(&path, "external edit\n").unwrap();
        let error = transaction.commit().unwrap_err();

        assert!(matches!(
            error,
            FileTransactionError::Changed { path: ref changed, .. } if changed == &path
        ));
        assert_eq!(fs::read_to_string(path).unwrap(), "external edit\n");
    }

    #[test]
    fn conflict_in_any_observation_prevents_the_first_publication() {
        let directory = tempfile::tempdir().unwrap();
        let first_path = directory.path().join("first.md");
        let second_path = directory.path().join("second.md");
        fs::write(&first_path, "first original\n").unwrap();
        fs::write(&second_path, "second original\n").unwrap();
        let mut transaction = FileTransaction::new();
        transaction
            .replace(
                snapshot(&first_path).unwrap(),
                Box::from(&b"first replacement\n"[..]),
            )
            .unwrap();
        transaction
            .replace(
                snapshot(&second_path).unwrap(),
                Box::from(&b"second replacement\n"[..]),
            )
            .unwrap();

        fs::write(&second_path, "external edit\n").unwrap();
        transaction.commit().unwrap_err();

        assert_eq!(fs::read_to_string(first_path).unwrap(), "first original\n");
        assert_eq!(fs::read_to_string(second_path).unwrap(), "external edit\n");
    }

    #[test]
    fn expectation_only_transaction_detects_changed_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("observed.md");
        fs::write(&path, "original\n").unwrap();
        let mut transaction = FileTransaction::new();
        transaction.expect(snapshot(&path).unwrap()).unwrap();

        fs::write(&path, "changed\n").unwrap();

        assert!(matches!(
            transaction.commit(),
            Err(FileTransactionError::Changed { .. })
        ));
        assert_eq!(fs::read_to_string(path).unwrap(), "changed\n");
    }

    #[test]
    fn rename_rejects_a_destination_created_after_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("source.md");
        let destination_path = directory.path().join("destination.md");
        fs::write(&source_path, "source\n").unwrap();
        let mut transaction = FileTransaction::new();
        transaction
            .rename(
                present(snapshot(&source_path).unwrap()),
                missing(snapshot(&destination_path).unwrap()),
            )
            .unwrap();

        fs::write(&destination_path, "occupied\n").unwrap();
        transaction.commit().unwrap_err();

        assert_eq!(fs::read_to_string(source_path).unwrap(), "source\n");
        assert_eq!(fs::read_to_string(destination_path).unwrap(), "occupied\n");
    }

    #[test]
    fn matching_snapshot_replaces_contents_and_preserves_permissions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("document.md");
        fs::write(&path, "original\n").unwrap();
        let permissions = fs::metadata(&path).unwrap().permissions();
        let mut transaction = FileTransaction::new();
        transaction
            .replace(snapshot(&path).unwrap(), Box::from(&b"replacement\n"[..]))
            .unwrap();

        transaction.commit().unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "replacement\n");
        assert_eq!(fs::metadata(path).unwrap().permissions(), permissions);
    }

    #[test]
    fn matching_missing_snapshot_permits_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("new.md");
        let mut transaction = FileTransaction::new();
        transaction
            .replace(
                snapshot(&path).unwrap(),
                Box::from(&b"created atomically\n"[..]),
            )
            .unwrap();

        transaction.commit().unwrap();

        assert_eq!(fs::read_to_string(path).unwrap(), "created atomically\n");
    }

    #[test]
    fn missing_replacement_rejects_a_newly_created_destination() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("new.md");
        let mut transaction = FileTransaction::new();
        transaction
            .replace(
                snapshot(&path).unwrap(),
                Box::from(&b"local creation\n"[..]),
            )
            .unwrap();
        fs::write(&path, "external creation\n").unwrap();

        let error = transaction.commit().unwrap_err();

        assert!(matches!(error, FileTransactionError::Changed { .. }));
        assert_eq!(fs::read_to_string(path).unwrap(), "external creation\n");
    }

    #[test]
    fn dropping_an_uncommitted_transaction_creates_no_temporary_sibling() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("document.md");
        fs::write(&path, "original\n").unwrap();
        let mut transaction = FileTransaction::new();
        transaction
            .replace(snapshot(&path).unwrap(), Box::from(&b"replacement\n"[..]))
            .unwrap();

        drop(transaction);

        assert_eq!(fs::read_to_string(&path).unwrap(), "original\n");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn second_mutation_for_one_path_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("document.md");
        fs::write(&path, "original\n").unwrap();
        let mut transaction = FileTransaction::new();
        transaction
            .replace(snapshot(&path).unwrap(), Box::from(&b"replacement\n"[..]))
            .unwrap();

        let error = transaction
            .remove(present(snapshot(&path).unwrap()))
            .unwrap_err();

        assert!(matches!(
            error,
            FileTransactionError::ConflictingIntent { path: ref conflict } if conflict == &path
        ));
        assert_eq!(fs::read_to_string(path).unwrap(), "original\n");
    }
}
