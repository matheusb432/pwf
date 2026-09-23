use std::{io, path::Path};

use pwf_application::ports::task_source_file::TaskSourceFileReader;

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalTaskSourceFileReader;

impl TaskSourceFileReader for LocalTaskSourceFileReader {
    fn read_task_source_file(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }
}
