use std::{io, path::PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum LocalAuthError {
    #[error("could not resolve a data directory (no PWF_DATA_DIR, no home)")]
    DataDirectoryUnavailable,
    #[error("PWF_DATA_DIR must be absolute: {}", path.display())]
    DataDirectoryRelative { path: PathBuf },
    #[error("local server path is a symbolic link: {}", path.display())]
    SymbolicLink { path: PathBuf },
    #[error("local server path is not a directory: {}", path.display())]
    NotDirectory { path: PathBuf },
    #[error("local server file is not a regular file: {}", path.display())]
    NotFile { path: PathBuf },
    #[error("local server path has unsafe ownership or permissions: {}", path.display())]
    UnsafePermissions { path: PathBuf },
    #[error("failed to create local server directory: {}", path.display())]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to inspect local server path: {}", path.display())]
    Inspect {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to open local server file: {}", path.display())]
    Open {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to read local server file: {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to write local server file: {}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to replace local server file: {}", path.display())]
    Replace {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to remove local server file: {}", path.display())]
    Remove {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to generate a local capability token")]
    GenerateToken(#[source] getrandom::Error),
    #[error("local capability token is malformed: {}", path.display())]
    MalformedToken { path: PathBuf },
    #[error("pwf-server has not published an endpoint at {}", path.display())]
    EndpointNotPublished { path: PathBuf },
    #[error("local server endpoint is malformed: {}", path.display())]
    MalformedEndpoint { path: PathBuf },
    #[error("local server endpoint must use a bound loopback address")]
    InvalidEndpoint,
}
