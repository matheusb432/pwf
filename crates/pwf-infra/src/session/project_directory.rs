//! Inspects managed-project paths through the local filesystem.

use pwf_application::ports::project_directory::ProjectDirectoryClient;

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalProjectDirectoryClient;

impl ProjectDirectoryClient for LocalProjectDirectoryClient {
    fn canonicalize(&self, path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
        let path = std::fs::canonicalize(path)?;
        #[cfg(windows)]
        let path = {
            use std::path::{Component, Prefix};
            let mut parts = path.components();
            if let Some(Component::Prefix(prefix)) = parts.next()
                && let Prefix::VerbatimDisk(drive) = prefix.kind()
            {
                let mut normalized = std::path::PathBuf::from(format!("{}:\\", char::from(drive)));
                normalized.extend(parts.skip(1));
                normalized
            } else {
                path
            }
        };
        Ok(path)
    }

    fn is_directory(&self, path: &std::path::Path) -> bool {
        path.is_dir()
    }
}
