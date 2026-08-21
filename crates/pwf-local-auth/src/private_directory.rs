use std::{
    fs::{DirBuilder, File, Metadata, OpenOptions},
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};

use crate::LocalAuthError;

#[derive(Debug)]
pub(crate) struct PrivateDirectory {
    path: PathBuf,
}

impl PrivateDirectory {
    pub(crate) fn open(path: PathBuf) -> Result<Self, LocalAuthError> {
        create_directory(&path)?;
        let metadata = inspect_path(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(LocalAuthError::SymbolicLink { path });
        }
        if !metadata.is_dir() {
            return Err(LocalAuthError::NotDirectory { path });
        }
        verify_private(&path, &metadata, PrivatePathKind::Directory)?;
        Ok(Self { path })
    }

    pub(crate) fn read(&self, name: &str) -> Result<Vec<u8>, LocalAuthError> {
        let path = self.path.join(name);
        let mut file = open_existing_private_file(&path)?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .map_err(|source| LocalAuthError::Read { path, source })?;
        Ok(contents)
    }

    pub(crate) fn create(&self, name: &str, contents: &[u8]) -> Result<bool, LocalAuthError> {
        let path = self.path.join(name);
        let mut options = private_open_options();
        options.write(true).create_new(true);
        let mut file = match options.open(&path) {
            Ok(file) => file,
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
            Err(source) => return Err(LocalAuthError::Open { path, source }),
        };
        file.write_all(contents)
            .and_then(|()| file.sync_all())
            .map_err(|source| LocalAuthError::Write {
                path: path.clone(),
                source,
            })?;
        verify_private(
            &path,
            &file.metadata().map_err(|source| LocalAuthError::Inspect {
                path: path.clone(),
                source,
            })?,
            PrivatePathKind::File,
        )?;
        Ok(true)
    }

    pub(crate) fn write_and_replace(
        &self,
        name: &str,
        contents: &[u8],
    ) -> Result<(), LocalAuthError> {
        let path = self.path.join(name);
        let temporary_name = format!(".{name}.{}.tmp", std::process::id());
        let temporary_path = self.path.join(&temporary_name);
        let _ = std::fs::remove_file(&temporary_path);
        if !self.create(&temporary_name, contents)? {
            return Err(LocalAuthError::Write {
                path: temporary_path,
                source: std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "temporary file already exists",
                ),
            });
        }

        #[cfg(windows)]
        if path.exists() {
            self.remove(name)?;
        }

        std::fs::rename(&temporary_path, &path).map_err(|source| LocalAuthError::Replace {
            path: path.clone(),
            source,
        })?;
        let metadata = inspect_path(&path)?;
        verify_private(&path, &metadata, PrivatePathKind::File)
    }

    pub(crate) fn remove(&self, name: &str) -> Result<(), LocalAuthError> {
        let path = self.path.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(LocalAuthError::Remove { path, source }),
        }
    }

    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

fn create_directory(path: &Path) -> Result<(), LocalAuthError> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|source| LocalAuthError::CreateDirectory {
            path: path.to_path_buf(),
            source,
        })
}

fn inspect_path(path: &Path) -> Result<Metadata, LocalAuthError> {
    std::fs::symlink_metadata(path).map_err(|source| LocalAuthError::Inspect {
        path: path.to_path_buf(),
        source,
    })
}

fn open_existing_private_file(path: &Path) -> Result<File, LocalAuthError> {
    let metadata = inspect_path(path)?;
    if metadata.file_type().is_symlink() {
        return Err(LocalAuthError::SymbolicLink {
            path: path.to_path_buf(),
        });
    }
    verify_private(path, &metadata, PrivatePathKind::File)?;
    private_open_options()
        .read(true)
        .open(path)
        .map_err(|source| LocalAuthError::Open {
            path: path.to_path_buf(),
            source,
        })
}

fn private_open_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    options
}

#[derive(Debug, Clone, Copy)]
enum PrivatePathKind {
    Directory,
    File,
}

fn verify_private(
    path: &Path,
    metadata: &Metadata,
    kind: PrivatePathKind,
) -> Result<(), LocalAuthError> {
    match kind {
        PrivatePathKind::Directory if !metadata.is_dir() => {
            return Err(LocalAuthError::NotDirectory {
                path: path.to_path_buf(),
            });
        }
        PrivatePathKind::File if !metadata.is_file() => {
            return Err(LocalAuthError::NotFile {
                path: path.to_path_buf(),
            });
        }
        PrivatePathKind::Directory | PrivatePathKind::File => {}
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;

        let expected_mode = match kind {
            PrivatePathKind::Directory => 0o700,
            PrivatePathKind::File => 0o600,
        };
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != expected_mode
            || matches!(kind, PrivatePathKind::File) && metadata.nlink() != 1
        {
            return Err(LocalAuthError::UnsafePermissions {
                path: path.to_path_buf(),
            });
        }
    }

    Ok(())
}
