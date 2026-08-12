//! Inspects managed-project paths through the local filesystem.

use std::path::Path;

use pwf_application::ports::project_directory::ProjectDirectoryClient;
use pwf_models::session::SessionWorkingDirectory;

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalProjectDirectoryClient;

impl ProjectDirectoryClient for LocalProjectDirectoryClient {
    fn is_directory(&self, path: &SessionWorkingDirectory) -> bool {
        Path::new(path.as_ref()).is_dir()
    }
}
