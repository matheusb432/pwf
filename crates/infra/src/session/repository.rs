//! Inspects repository paths through the local filesystem.

use std::path::Path;

use pwf_application::RepositoryDirectoryClient;

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalRepositoryClient;

impl RepositoryDirectoryClient for LocalRepositoryClient {
    fn is_directory(&self, path: &str) -> bool {
        Path::new(path).is_dir()
    }
}
