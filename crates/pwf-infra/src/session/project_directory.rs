//! Inspects managed-project paths through the local filesystem.

use std::path::Path;

use pwf_application::ports::project_directory::ProjectDirectoryClient;

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalProjectDirectoryClient;

impl ProjectDirectoryClient for LocalProjectDirectoryClient {
    fn is_directory(&self, path: &str) -> bool {
        Path::new(path).is_dir()
    }
}
