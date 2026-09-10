mod identity;
mod markdown_file;
mod markdown_line;
mod note_frontmatter;
mod note_text;
mod project_rename;
mod store;
mod trash;

pub use identity::{TaskNoteIdentity, inspect_project_task_notes};
pub use markdown_file::{
    FrontmatterParseError, FrontmatterSerializeError, FrontmatterView, MarkdownFile,
    MarkdownFileError,
};
pub use project_rename::ObsidianProjectTaskFilesClient;
pub use store::{ObsidianStore, ObsidianStoreError};

pub(super) const PROJECT_SNAPSHOT_FILE_NAME: &str = "pwf-index.md";
