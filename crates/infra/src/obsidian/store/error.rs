use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ObsidianStoreError {
    #[error("Project rename destination already exists: {}", path.display())]
    ProjectRenameDestinationExists { path: PathBuf },
    #[error("Project rename staging directory already exists: {}", path.display())]
    ProjectRenameStagingExists { path: PathBuf },
    #[error("Project rename backup directory already exists: {}", path.display())]
    ProjectRenameBackupExists { path: PathBuf },
    #[error("Cannot inspect project rename path {}: {source}", path.display())]
    InspectProjectRenamePath {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Project rename source is not a directory: {}", path.display())]
    ProjectRenameSourceNotDirectory { path: PathBuf },
    #[error("Project rename source has no directory name: {}", path.display())]
    ProjectRenameSourceNameMissing { path: PathBuf },
    #[error("Project rename source has no parent directory: {}", path.display())]
    ProjectRenameSourceParentMissing { path: PathBuf },
    #[error("Cannot create project rename staging directory {}: {source}", path.display())]
    CreateProjectRenameStaging {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot walk project rename source {}: {source}", path.display())]
    WalkProjectRenameSource {
        path: PathBuf,
        source: walkdir::Error,
    },
    #[error("Project rename source exceeds {limit} entries: {}", path.display())]
    ProjectRenameEntryLimit { path: PathBuf, limit: usize },
    #[error("Project rename source exceeds depth {limit}: {}", path.display())]
    ProjectRenameDepthLimit { path: PathBuf, limit: usize },
    #[error("Project rename source contains an unsupported filesystem entry: {}", path.display())]
    ProjectRenameEntryUnsupported { path: PathBuf },
    #[error("Cannot create staged project rename directory {}: {source}", path.display())]
    CreateStagedProjectRenameDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot copy project rename entry from {} to {}: {source}", from.display(), to.display())]
    CopyProjectRenameEntry {
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot read staged project rename file {}: {source}", path.display())]
    ReadStagedProjectRenameFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Staged project rename file is not UTF-8 Markdown: {}", path.display())]
    StagedProjectRenameFileNotUtf8 { path: PathBuf },
    #[error("Cannot write staged project rename file {}: {source}", path.display())]
    WriteStagedProjectRenameFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Project rename file already exists: {}", path.display())]
    ProjectRenameFileExists { path: PathBuf },
    #[error("Cannot rename staged project file from {} to {}: {source}", from.display(), to.display())]
    RenameStagedProjectFile {
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot remove project rename staging directory {}: {source}", path.display())]
    RemoveProjectRenameStaging {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot move project rename source from {} to {}: {source}", from.display(), to.display())]
    MoveProjectRenameSource {
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot install staged project rename from {} to {}: {source}", from.display(), to.display())]
    InstallStagedProjectRename {
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot restore project rename source {} after staging install from {} to {} failed ({install_source}): {restore_source}", source_path.display(), from.display(), to.display())]
    RestoreProjectRenameSource {
        source_path: PathBuf,
        from: PathBuf,
        to: PathBuf,
        install_source: std::io::Error,
        restore_source: std::io::Error,
    },
    #[error("Cannot remove project rename staging directory {} after restoring the source at {} because installing it at {} failed ({install_source}): {cleanup_source}", path.display(), from.display(), to.display())]
    RemoveRestoredProjectRenameStaging {
        path: PathBuf,
        from: PathBuf,
        to: PathBuf,
        install_source: std::io::Error,
        cleanup_source: std::io::Error,
    },
    #[error("Cannot read note {id}: {source}")]
    ReadProjectNote { id: String, source: std::io::Error },
    #[error("Cannot inspect note {id}: {source}")]
    InspectProjectNote { id: String, source: std::io::Error },
    #[error("Cannot write note {id}: {source}")]
    WriteProjectNote { id: String, source: std::io::Error },
    #[error("Cannot remove note {id}: {source}")]
    RemoveProjectNote { id: String, source: std::io::Error },
    #[error("No such note {id} in {project}.")]
    ProjectNoteNotFound { id: String, project: String },
    #[error("Cannot write index: {source}")]
    WriteProjectNoteIndex {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot read handoff directory {}: {source}", path.display())]
    ReadHandoffDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot read handoff document {}: {source}", path.display())]
    ReadHandoffDocument {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot read handoff metadata {}: {source}", path.display())]
    ReadHandoffMetadata {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot inspect handoff document {}: {source}", path.display())]
    InspectHandoffDocument {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Handoff document already exists: {}", path.display())]
    HandoffDocumentExists { path: PathBuf },
    #[error("Cannot write handoff document {}: {source}", path.display())]
    WriteHandoffDocument {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot create handoff archive directory {}: {source}", path.display())]
    CreateHandoffArchiveDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot move handoff document from {} to {}: {source}", from.display(), to.display())]
    MoveHandoffDocument {
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot restore handoff document {} after move from {} to {} failed ({move_source}): {restore_source}", path.display(), from.display(), to.display())]
    RestoreHandoffDocument {
        path: PathBuf,
        from: PathBuf,
        to: PathBuf,
        move_source: std::io::Error,
        restore_source: std::io::Error,
    },
    #[error("Cannot remove handoff document {}: {source}", path.display())]
    RemoveHandoffDocument {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot read handoff ledger {}: {source}", path.display())]
    ReadHandoffLedger {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{source}")]
    WriteHandoffLedger {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot remove handoff ledger {}: {source}", path.display())]
    RemoveHandoffLedger {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot parse frontmatter property `{property}` in {}: {source}", path.display())]
    FrontmatterParse {
        path: PathBuf,
        property: &'static str,
        source: gray_matter::Error,
    },
    #[error("Missing frontmatter property `{property}` in {}", path.display())]
    MissingFrontmatter {
        path: PathBuf,
        property: &'static str,
    },
    #[error("Missing task frontmatter property `id` in {}", path.display())]
    MissingTaskId { path: PathBuf },
    #[error("Invalid task frontmatter property `id` {value:?} in {}", path.display())]
    InvalidTaskId { path: PathBuf, value: String },
    #[error("More than one task has frontmatter id {id}: {}", paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", "))]
    DuplicateTaskId { id: String, paths: Vec<PathBuf> },
    #[error("Project index task id {id} is duplicated in {} at lines {}", path.display(), lines.iter().map(usize::to_string).collect::<Vec<_>>().join(", "))]
    ProjectIndexTaskIdDuplicate {
        path: PathBuf,
        id: String,
        lines: Vec<usize>,
    },
    #[error("Missing project-index frontmatter property `{property}` in {}", path.display())]
    MissingProjectIndexProperty {
        path: PathBuf,
        property: &'static str,
    },
    #[error(
        "Invalid project-index frontmatter property `{property}` {value:?} in {}",
        path.display()
    )]
    InvalidProjectIndexProperty {
        path: PathBuf,
        property: &'static str,
        value: String,
    },
    #[error(
        "Project-index identity mismatch in {}: found id={actual_id:?}, title={actual_title:?}; expected id={expected_id:?}, title={expected_title:?}",
        path.display()
    )]
    ProjectIndexIdentityMismatch {
        path: PathBuf,
        actual_id: String,
        actual_title: String,
        expected_id: String,
        expected_title: String,
    },
    #[error("Notes directory not found: {path}")]
    NotesDirectoryNotFound { path: String },
    #[error("Unknown project '{project}'.")]
    UnknownProject { project: String },
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
    #[error("update only supports file-model pending-work items.")]
    UpdateRequiresFileModel,
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
    #[error(
        "index sections are managed implicitly by index-entry writes; direct {op} is unsupported."
    )]
    IndexSectionWriteUnsupported { op: &'static str },
}

impl ObsidianStoreError {
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
