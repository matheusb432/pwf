//! Inspects repository paths through the local filesystem.

use std::path::Path;

use pwf_application::pending_work::session::RepositorySessionClient;

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalRepositoryClient;

impl RepositorySessionClient for LocalRepositoryClient {
    fn is_directory(&self, path: &str) -> bool {
        Path::new(path).is_dir()
    }
}
