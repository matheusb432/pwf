use std::{io, path::Path};

/// Reads authored task source files without exposing filesystem details to interactors.
pub trait TaskSourceFileReader: Send + Sync + 'static {
    fn read_task_source_file(&self, path: &Path) -> io::Result<String>;
}
