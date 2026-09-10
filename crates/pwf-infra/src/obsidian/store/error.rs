use std::path::PathBuf;

use pwf_models::{
    note::NoteTitleError,
    project::ProjectId,
    task::{ParseTaskStatusError, TaskId, TaskTimestampError},
};

#[derive(Debug, thiserror::Error)]
pub enum ObsidianStoreError {
    #[error("The authored project page is reserved: {}", path.display())]
    ProjectPagePathReserved { path: PathBuf },
    #[error("Cannot write project snapshot {}: {source}", path.display())]
    WriteProjectSnapshot {
        path: PathBuf,
        source: std::io::Error,
    },
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
    #[error("Cannot read project notes directory {}: {source}", path.display())]
    ReadProjectNoteDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Invalid note {id} title: {source}")]
    InvalidProjectNoteTitle {
        id: String,
        #[source]
        source: NoteTitleError,
    },
    #[error("Cannot write note {id}: {source}")]
    WriteProjectNote { id: String, source: std::io::Error },
    #[error("Cannot edit note {id}: {source}")]
    EditProjectNote {
        id: String,
        #[source]
        source: crate::obsidian::MarkdownFileError,
    },
    #[error("Invalid note {id} tag at index {index}: {source}")]
    InvalidProjectNoteTag {
        id: String,
        index: usize,
        #[source]
        source: pwf_models::note::NoteTagError,
    },
    #[error("Invalid note {id} source at index {index}: {source}")]
    InvalidProjectNoteSource {
        id: String,
        index: usize,
        #[source]
        source: pwf_models::note::NoteSourceError,
    },
    #[error("Cannot remove note {id}: {source}")]
    RemoveProjectNote { id: String, source: std::io::Error },
    #[error("No such note {id} in {project}.")]
    ProjectNoteNotFound { id: String, project: String },
    #[error("Cannot parse frontmatter property `{property}` in {}: {source}", path.display())]
    FrontmatterParse {
        path: PathBuf,
        property: &'static str,
        source: crate::obsidian::MarkdownFileError,
    },
    #[error("Missing frontmatter property `{property}` in {}", path.display())]
    MissingFrontmatter {
        path: PathBuf,
        property: &'static str,
    },
    #[error("Invalid task frontmatter property `id` {value:?} in {}", path.display())]
    InvalidTaskId { path: PathBuf, value: String },
    #[error("Invalid task frontmatter property `{property}` {value:?} in {}: {source}", path.display())]
    InvalidTaskTimestamp {
        path: PathBuf,
        property: &'static str,
        value: String,
        #[source]
        source: TaskTimestampError,
    },
    #[error("Invalid task frontmatter property `status` {value:?} in {}: {source}", path.display())]
    InvalidTaskStatus {
        path: PathBuf,
        value: String,
        #[source]
        source: ParseTaskStatusError,
    },
    #[error("More than one task has frontmatter id {id}: {}", paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", "))]
    DuplicateTaskId { id: TaskId, paths: Vec<PathBuf> },
    #[error("Invalid task path for project '{project}': {source}")]
    InvalidProjectTaskPath {
        project: String,
        #[source]
        source: pwf_application::project::runtime_path::RuntimePathError,
    },
    #[error("Cannot create project dir: {source}")]
    CreateProjectDir { source: std::io::Error },
    #[error("Cannot read task file: {source}")]
    ReadTaskFile { source: std::io::Error },
    #[error("Cannot write task file: {source}")]
    WriteTaskFile { source: std::io::Error },
    #[error("Failed to write task file: {source}")]
    AddWriteTaskFile { source: std::io::Error },
    #[error("Task file path has no file name: {}", path.display())]
    TaskFileNameMissing { path: PathBuf },
    #[error("task deletion destination does not match the registered project vault")]
    TaskDeletionChanged,
    #[error("invalid registered Obsidian vault path: {source}")]
    TaskVaultPath {
        #[source]
        source: pwf_application::project::runtime_path::RuntimePathError,
    },
    #[error("Obsidian trash folder must exist and be a directory: {}", path.display())]
    TaskTrashDirectory { path: PathBuf },
    #[error("Obsidian trash destination already exists: {}", path.display())]
    TaskTrashDestinationExists { path: PathBuf },
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error("Cannot commit guarded task files: {source}")]
    TaskMutationFilesystem {
        #[source]
        source: std::io::Error,
    },
    #[error("Task ID sequence is exhausted for project {project_id}")]
    TaskIdSequenceExhausted { project_id: ProjectId },
    #[error("task ID {id} is already occupied at {}; retry the command", path.display())]
    TaskIdOccupied { id: TaskId, path: PathBuf },
}
