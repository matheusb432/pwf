use std::path::PathBuf;

use pwf_models::{
    project::{ProjectId, ProjectName},
    task::TaskId,
};

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
    DuplicateTaskId { id: TaskId, paths: Vec<PathBuf> },
    #[error("Project index task id {id} is duplicated in {} at lines {}", path.display(), lines.iter().map(usize::to_string).collect::<Vec<_>>().join(", "))]
    ProjectIndexTaskIdDuplicate {
        path: PathBuf,
        id: TaskId,
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
        "Project-index identity mismatch in {}: found id={actual_id}, title={actual_title}; expected id={expected_id}, title={expected_title}",
        path.display()
    )]
    ProjectIndexIdentityMismatch {
        path: PathBuf,
        actual_id: ProjectId,
        actual_title: ProjectName,
        expected_id: ProjectId,
        expected_title: ProjectName,
    },
    #[error("Notes directory not found: {path}")]
    NotesDirectoryNotFound { path: String },
    #[error("Invalid task path for project '{project}': {source}")]
    InvalidProjectTaskPath {
        project: String,
        #[source]
        source: pwf_application::project::resolve_runtime_path::RuntimePathError,
    },
    #[error("Cannot create project dir: {source}")]
    CreateProjectDir { source: std::io::Error },
    #[error("Cannot create index dir: {source}")]
    CreateIndexDir { source: std::io::Error },
    #[error("Cannot read task file: {source}")]
    ReadTaskFile { source: std::io::Error },
    #[error("Cannot read index: {source}")]
    ReadIndex { source: std::io::Error },
    #[error("Cannot write task file: {source}")]
    WriteTaskFile { source: std::io::Error },
    #[error("Cannot write index: {source}")]
    WriteIndex { source: std::io::Error },
    #[error("Failed to write task file: {source}")]
    AddWriteTaskFile { source: std::io::Error },
    #[error("Failed to write index file: {source}")]
    AddWriteIndexFile {
        source: std::io::Error,
        project: String,
        created_section: Option<String>,
    },
    #[error("Cannot remove task file: {source}")]
    RemoveTaskFile { source: std::io::Error },
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error("Task ID sequence is exhausted for project {project_id}")]
    TaskIdSequenceExhausted { project_id: ProjectId },
    #[error("Expected open task marker at {note}:{line}. The note may have changed.")]
    ExpectedOpenTaskMarker { note: String, line: usize },
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
