mod identity;
mod markdown_file;
mod markdown_line;
mod note_frontmatter;
mod note_text;
mod project_rename;
mod store;
mod trash;

use std::path::{Path, PathBuf};

pub use markdown_file::{
    FrontmatterParseError, FrontmatterSerializeError, FrontmatterView, MarkdownFile,
    MarkdownFileError,
};
pub use project_rename::ObsidianProjectTaskFilesClient;
pub use store::{ObsidianStore, ObsidianStoreError};

fn project_snapshot_path(project_directory: &Path) -> Option<PathBuf> {
    let mut file_name = project_directory.file_name()?.to_os_string();
    file_name.push(".md");
    Some(project_directory.join(file_name))
}

fn project_snapshot_backup_path(project_directory: &Path) -> Option<PathBuf> {
    project_snapshot_path(project_directory).map(|path| path.with_extension("backup.md"))
}
