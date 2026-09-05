use std::{
    io,
    os::unix::fs::{DirBuilderExt as _, MetadataExt as _},
    path::{Path, PathBuf},
    time::Duration,
};

use tonic::transport::{Channel, Endpoint};

pub mod listener;

#[derive(Debug, Clone)]
pub struct LocalEndpoint {
    root: PathBuf,
    path: String,
}

impl LocalEndpoint {
    pub fn from_environment() -> io::Result<Self> {
        let root =
            std::env::var_os("PWF_RUNTIME_DIR").map_or_else(default_runtime_root, PathBuf::from);
        Self::from_root(root)
    }

    /// Validates an absolute runtime directory and the platform socket-path capacity.
    pub fn from_root(root: impl AsRef<Path>) -> io::Result<Self> {
        let root = root.as_ref();
        if !root.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "PWF_RUNTIME_DIR must be absolute",
            ));
        }
        let path = root
            .join("pwf.sock")
            .into_os_string()
            .into_string()
            .map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "socket path must be UTF-8")
            })?;
        // SAFETY: a zeroed sockaddr_un contains only integer fields and a byte array.
        let address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        if path.as_bytes().contains(&0) || path.len() >= address.sun_path.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "socket path contains NUL or exceeds the platform capacity",
            ));
        }
        Ok(Self {
            root: root.to_path_buf(),
            path,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        Path::new(&self.path)
    }

    pub async fn connect(
        &self,
        timeout: Duration,
        request_timeout: Duration,
    ) -> Result<Channel, crate::ConnectError> {
        inspect_directory(self.directory())?;
        let channel = Endpoint::from_shared(format!("unix:{}", self.path))?
            .connect_timeout(timeout)
            .timeout(request_timeout)
            .connect()
            .await?;
        Ok(channel)
    }

    fn directory(&self) -> &Path {
        &self.root
    }

    fn prepare_directory(&self) -> io::Result<()> {
        match std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(self.directory())
        {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        inspect_directory(self.directory())
    }
}

fn default_runtime_root() -> PathBuf {
    directories::ProjectDirs::from("", "", "pwf")
        .and_then(|directories| directories.runtime_dir().map(Path::to_path_buf))
        .unwrap_or_else(|| std::env::temp_dir().join(format!("pwf-{}", effective_user_id())))
}

fn effective_user_id() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}

fn inspect_directory(path: &Path) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.uid() != effective_user_id()
        || metadata.mode() & 0o777 != 0o700
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "runtime directory must be owned by the current user with mode 0700: {}",
                path.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    #[test]
    fn invalid_paths_do_not_create_directories() {
        assert!(LocalEndpoint::from_root("relative").is_err());
        let directory = tempfile::tempdir().unwrap();
        let long = directory.path().join("x".repeat(256));
        assert!(LocalEndpoint::from_root(&long).is_err());
        assert!(!long.exists());
        assert!(LocalEndpoint::from_root(directory.path().join("nul\0")).is_err());
    }

    #[tokio::test]
    async fn listener_rejects_unsafe_or_symlinked_parent() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("runtime");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        let endpoint = LocalEndpoint::from_root(&root).unwrap();
        assert!(
            listener::LocalListener::bind(&endpoint, Duration::from_millis(100))
                .await
                .is_err()
        );
        std::fs::remove_dir(&root).unwrap();
        std::os::unix::fs::symlink(directory.path(), &root).unwrap();
        assert!(
            listener::LocalListener::bind(&endpoint, Duration::from_millis(100))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn listener_rejects_non_socket_and_symlink() {
        let directory = tempfile::tempdir().unwrap();
        let endpoint = LocalEndpoint::from_root(directory.path()).unwrap();
        std::fs::write(endpoint.path(), "keep").unwrap();
        assert!(
            listener::LocalListener::bind(&endpoint, Duration::from_millis(100))
                .await
                .is_err()
        );
        assert_eq!(std::fs::read_to_string(endpoint.path()).unwrap(), "keep");
        std::fs::remove_file(endpoint.path()).unwrap();
        let target = directory.path().join("target");
        std::fs::write(&target, "keep").unwrap();
        std::os::unix::fs::symlink(&target, endpoint.path()).unwrap();
        assert!(
            listener::LocalListener::bind(&endpoint, Duration::from_millis(100))
                .await
                .is_err()
        );
        assert_eq!(std::fs::read_to_string(target).unwrap(), "keep");
    }
}
