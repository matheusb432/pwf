use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ObsidianPendingWorkStoreError {
    #[error("Notes directory not found: {path}")]
    NotesDirectoryNotFound { path: String },
    #[error("Project '{project}' is not mapped to a repo in config/pending-work.json.")]
    ProjectNotMappedToRepo { project: String },
    #[error("Project '{project}' has no work-item prefix in config/pending-work.json (prefixes).")]
    ProjectMissingPrefix { project: String },
    #[error("Cannot create project dir: {source}")]
    CreateProjectDir { source: std::io::Error },
    #[error("Cannot create index dir: {source}")]
    CreateIndexDir { source: std::io::Error },
    #[error("Cannot read item file: {source}")]
    ReadItemFile { source: std::io::Error },
    #[error("Cannot read index: {source}")]
    ReadIndex { source: std::io::Error },
    #[error("Cannot write item file: {source}")]
    WriteItemFile { source: std::io::Error },
    #[error("Cannot write index: {source}")]
    WriteIndex { source: std::io::Error },
    #[error("Failed to write item file: {source}")]
    AddWriteItemFile { source: std::io::Error },
    #[error("Failed to write index file: {source}")]
    AddWriteIndexFile {
        source: std::io::Error,
        project: String,
        created_section: Option<String>,
    },
    #[error("Cannot remove item file: {source}")]
    RemoveItemFile { source: std::io::Error },
    #[error("Cannot create archive dir: {source}")]
    CreateArchiveDir { source: std::io::Error },
    #[error("Cannot archive {id}: {source}")]
    ArchiveItem { id: String, source: std::io::Error },
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    #[error("Pending-work id is ambiguous: {id}")]
    AmbiguousId { id: String },
    #[error("remove only supports file-model pending-work items.")]
    RemoveRequiresFileModel,
    #[error("update only supports file-model pending-work items.")]
    UpdateRequiresFileModel,
    #[error("Work-item note missing: {}", path.display())]
    WorkItemNoteMissing { path: PathBuf },
    #[error("Index link not found for {id}.")]
    IndexLinkNotFound { id: String },
    #[error(
        "nothing to update (pass --prompt, --title, --prereq, --clear-prereq, --tag, --tags-clear, --commits, --append-report, --append, and/or --effort)."
    )]
    NothingToUpdate,
    #[error(
        "only --commits / --append-report can amend closed item {id} (done/cancelled); body/title/prereq/tags/append/effort need an open item."
    )]
    ClosedItemAmendOnly { id: String },
    #[error("item {id} has invalid tags frontmatter: {raw:?}.")]
    InvalidTagsFrontmatter { id: String, raw: String },
    #[error("Invalid --prereq id: {raw}.")]
    InvalidPrereqId { raw: String },
    #[error("--prereq requires an id.")]
    MissingPrereqId,
    #[error("Unknown --prereq id(s): {}.", ids.join(", "))]
    UnknownPrereqIds { ids: Vec<String> },
    #[error("--report cannot be empty.")]
    EmptyReport,
    #[error("--append cannot be empty.")]
    EmptyAppend,
    #[error("Expected open task marker at {note}:{line}. The note may have changed.")]
    ExpectedOpenTaskMarker { note: String, line: usize },
}

impl ObsidianPendingWorkStoreError {
    pub fn created_section_diagnostic(&self) -> Option<(&str, &str)> {
        match self {
            Self::AddWriteIndexFile {
                project,
                created_section: Some(section),
                ..
            } => Some((project.as_str(), section.as_str())),
            _ => None,
        }
    }
}
