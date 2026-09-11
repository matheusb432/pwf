use std::{
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
};

use pwf_application::ports::project_task_files::{
    ProjectTaskFilesClient, ProjectTaskFilesRenameCommit, StagedProjectTaskFilesRename,
};
use pwf_models::project::ProjectIdentity;
use walkdir::{DirEntry, WalkDir};

use super::{MarkdownFile, ObsidianStoreError, PROJECT_SNAPSHOT_FILE_NAME};

const DIRECTORY_DEPTH_MAX: usize = 64;
const ENTRY_COUNT_MAX: usize = 100_000;

/// Adapts Obsidian project directories to application-owned task-file rename operations.
#[derive(Clone, Copy, Debug, Default)]
pub struct ObsidianProjectTaskFilesClient;

/// Holds a copied and rewritten project directory until both stores are ready to commit.
pub struct StagedProjectRename {
    source: PathBuf,
    destination: PathBuf,
    staging_directory: PathBuf,
    backup_directory: PathBuf,
}

impl ProjectTaskFilesClient for ObsidianProjectTaskFilesClient {
    type Error = ObsidianStoreError;
    type StagedRename = StagedProjectRename;

    fn stage_project_rename(
        &self,
        source: &Path,
        destination: &Path,
        current: &ProjectIdentity,
        next: &ProjectIdentity,
    ) -> Result<Self::StagedRename, Self::Error> {
        stage(source, destination, current, next)
    }
}

/// Copies and rewrites one project directory without changing the source.
fn stage(
    source: &Path,
    destination: &Path,
    current: &ProjectIdentity,
    next: &ProjectIdentity,
) -> Result<StagedProjectRename, ObsidianStoreError> {
    reject_existing_destination(destination)?;
    let source_name =
        source
            .file_name()
            .ok_or_else(|| ObsidianStoreError::ProjectRenameSourceNameMissing {
                path: source.to_path_buf(),
            })?;
    let parent =
        source
            .parent()
            .ok_or_else(|| ObsidianStoreError::ProjectRenameSourceParentMissing {
                path: source.to_path_buf(),
            })?;
    let staging_directory = parent.join(sibling_name(source_name, ".pwf-rename-staging"));
    let backup_directory = parent.join(sibling_name(source_name, ".pwf-rename-backup"));
    reject_existing_staging(&staging_directory)?;
    reject_existing_backup(&backup_directory)?;
    let metadata = fs::symlink_metadata(source).map_err(|source_error| {
        ObsidianStoreError::InspectProjectRenamePath {
            path: source.to_path_buf(),
            source: source_error,
        }
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ObsidianStoreError::ProjectRenameSourceNotDirectory {
            path: source.to_path_buf(),
        });
    }

    fs::create_dir(&staging_directory).map_err(|source_error| {
        ObsidianStoreError::CreateProjectRenameStaging {
            path: staging_directory.clone(),
            source: source_error,
        }
    })?;
    let staged = StagedProjectRename {
        source: source.to_path_buf(),
        destination: destination.to_path_buf(),
        staging_directory,
        backup_directory,
    };
    if let Err(error) = copy_and_rewrite(&staged, current, next) {
        return match fs::remove_dir_all(&staged.staging_directory) {
            Ok(()) => Err(error),
            Err(source_error) => Err(ObsidianStoreError::RemoveProjectRenameStaging {
                path: staged.staging_directory,
                source: source_error,
            }),
        };
    }
    Ok(staged)
}

impl StagedProjectTaskFilesRename for StagedProjectRename {
    type Error = ObsidianStoreError;

    fn commit(self) -> Result<ProjectTaskFilesRenameCommit, Self::Error> {
        self.commit_with_backup_removal(|path| fs::remove_dir_all(path))
    }

    fn discard(self) -> Result<(), Self::Error> {
        fs::remove_dir_all(&self.staging_directory).map_err(|source_error| {
            ObsidianStoreError::RemoveProjectRenameStaging {
                path: self.staging_directory,
                source: source_error,
            }
        })
    }
}

impl StagedProjectRename {
    fn commit_with_backup_removal(
        self,
        remove_backup: impl FnOnce(&Path) -> io::Result<()>,
    ) -> Result<ProjectTaskFilesRenameCommit, ObsidianStoreError> {
        commit_staged_rename(self, remove_backup)
    }
}

fn commit_staged_rename(
    staged: StagedProjectRename,
    remove_backup: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<ProjectTaskFilesRenameCommit, ObsidianStoreError> {
    reject_existing_destination(&staged.destination)?;
    reject_existing_backup(&staged.backup_directory)?;
    if let Err(source) = fs::rename(&staged.source, &staged.backup_directory) {
        let _ = fs::remove_dir_all(&staged.staging_directory);
        return Err(ObsidianStoreError::MoveProjectRenameSource {
            from: staged.source,
            to: staged.backup_directory,
            source,
        });
    }
    if let Err(source) = fs::rename(&staged.staging_directory, &staged.destination) {
        return rollback_failed_install(staged, source);
    }
    match remove_backup(&staged.backup_directory) {
        Ok(()) => Ok(ProjectTaskFilesRenameCommit::Complete),
        Err(source) => Ok(ProjectTaskFilesRenameCommit::BackupRetained {
            path: staged.backup_directory,
            source: anyhow::Error::new(source),
        }),
    }
}

fn rollback_failed_install(
    staged: StagedProjectRename,
    install_source: io::Error,
) -> Result<ProjectTaskFilesRenameCommit, ObsidianStoreError> {
    if let Err(restore_source) = fs::rename(&staged.backup_directory, &staged.source) {
        return Err(ObsidianStoreError::RestoreProjectRenameSource {
            source_path: staged.source,
            from: staged.staging_directory,
            to: staged.destination,
            install_source,
            restore_source,
        });
    }
    if let Err(cleanup_source) = fs::remove_dir_all(&staged.staging_directory) {
        return Err(ObsidianStoreError::RemoveRestoredProjectRenameStaging {
            path: staged.staging_directory,
            from: staged.source,
            to: staged.destination,
            install_source,
            cleanup_source,
        });
    }
    Err(ObsidianStoreError::InstallStagedProjectRename {
        from: staged.staging_directory,
        to: staged.destination,
        source: install_source,
    })
}

fn copy_and_rewrite(
    staged: &StagedProjectRename,
    current: &ProjectIdentity,
    next: &ProjectIdentity,
) -> Result<(), ObsidianStoreError> {
    copy_source(staged)?;
    rewrite_markdown(&staged.staging_directory, current, next)
}

fn copy_source(staged: &StagedProjectRename) -> Result<(), ObsidianStoreError> {
    for (index, entry) in WalkDir::new(&staged.source)
        .min_depth(1)
        .max_depth(DIRECTORY_DEPTH_MAX + 1)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .enumerate()
    {
        copy_source_entry(staged, index, entry)?;
    }
    Ok(())
}

fn copy_source_entry(
    staged: &StagedProjectRename,
    index: usize,
    entry: Result<DirEntry, walkdir::Error>,
) -> Result<(), ObsidianStoreError> {
    if index >= ENTRY_COUNT_MAX {
        return Err(ObsidianStoreError::ProjectRenameEntryLimit {
            path: staged.source.clone(),
            limit: ENTRY_COUNT_MAX,
        });
    }
    let entry = entry.map_err(|source| ObsidianStoreError::WalkProjectRenameSource {
        path: staged.source.clone(),
        source,
    })?;
    if entry.depth() > DIRECTORY_DEPTH_MAX {
        return Err(ObsidianStoreError::ProjectRenameDepthLimit {
            path: entry.path().to_path_buf(),
            limit: DIRECTORY_DEPTH_MAX,
        });
    }
    let relative = entry.path().strip_prefix(&staged.source).map_err(|_| {
        ObsidianStoreError::ProjectRenameEntryUnsupported {
            path: entry.path().to_path_buf(),
        }
    })?;
    let destination = staged.staging_directory.join(relative);
    if entry.file_type().is_dir() {
        return fs::create_dir(&destination).map_err(|source| {
            ObsidianStoreError::CreateStagedProjectRenameDirectory {
                path: destination,
                source,
            }
        });
    }
    if entry.file_type().is_file() {
        return fs::copy(entry.path(), &destination)
            .map(|_| ())
            .map_err(|source| ObsidianStoreError::CopyProjectRenameEntry {
                from: entry.path().to_path_buf(),
                to: destination,
                source,
            });
    }
    Err(ObsidianStoreError::ProjectRenameEntryUnsupported {
        path: entry.path().to_path_buf(),
    })
}

fn rewrite_markdown(
    staging_directory: &Path,
    current: &ProjectIdentity,
    next: &ProjectIdentity,
) -> Result<(), ObsidianStoreError> {
    let mut markdown_paths = Vec::new();
    for (index, entry) in WalkDir::new(staging_directory)
        .min_depth(1)
        .max_depth(DIRECTORY_DEPTH_MAX)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .enumerate()
    {
        if index >= ENTRY_COUNT_MAX {
            return Err(ObsidianStoreError::ProjectRenameEntryLimit {
                path: staging_directory.to_path_buf(),
                limit: ENTRY_COUNT_MAX,
            });
        }
        let entry = entry.map_err(|source| ObsidianStoreError::WalkProjectRenameSource {
            path: staging_directory.to_path_buf(),
            source,
        })?;
        if entry.file_type().is_file()
            && entry.path().extension().and_then(OsStr::to_str) == Some("md")
        {
            markdown_paths.push(entry.path().to_path_buf());
        }
    }

    let mut renames = Vec::new();
    for path in &markdown_paths {
        if let Some(rename) = plan_markdown_rename(path, current, next)? {
            renames.push(rename);
        }
    }
    for path in markdown_paths {
        if path != staging_directory.join(format!("{}.md", current.title()))
            && path != staging_directory.join(PROJECT_SNAPSHOT_FILE_NAME)
        {
            rewrite_markdown_file(&path, current, next)?;
        }
    }
    for (source, destination) in renames {
        fs::rename(&source, &destination).map_err(|source_error| {
            ObsidianStoreError::RenameStagedProjectFile {
                from: source,
                to: destination,
                source: source_error,
            }
        })?;
    }
    Ok(())
}

fn plan_markdown_rename(
    path: &Path,
    current: &ProjectIdentity,
    next: &ProjectIdentity,
) -> Result<Option<(PathBuf, PathBuf)>, ObsidianStoreError> {
    let Some(name) = renamed_markdown_name(path, current, next) else {
        return Ok(None);
    };
    let destination = path.with_file_name(name);
    if destination != path && destination.exists() {
        return Err(ObsidianStoreError::ProjectRenameFileExists { path: destination });
    }
    Ok(Some((path.to_path_buf(), destination)))
}

fn rewrite_markdown_file(
    path: &Path,
    current: &ProjectIdentity,
    next: &ProjectIdentity,
) -> Result<(), ObsidianStoreError> {
    let mut file = MarkdownFile::open(path).map_err(|source| {
        let source = source.into_io_error();
        if source.kind() == io::ErrorKind::InvalidData {
            ObsidianStoreError::StagedProjectRenameFileNotUtf8 {
                path: path.to_path_buf(),
            }
        } else {
            ObsidianStoreError::ReadStagedProjectRenameFile {
                path: path.to_path_buf(),
                source,
            }
        }
    })?;
    let rewritten = rewrite_markdown_text(file.source(), current, next);
    if rewritten != file.source() {
        file.replace_source(rewritten);
        file.save()
            .map_err(|source| ObsidianStoreError::WriteStagedProjectRenameFile {
                path: path.to_path_buf(),
                source: source.into_io_error(),
            })?;
    }
    Ok(())
}

fn rewrite_markdown_text(
    markdown: &str,
    current: &ProjectIdentity,
    next: &ProjectIdentity,
) -> String {
    let markers = MarkdownRewriteMarkers::new(current, next);
    let mut state = MarkdownRewriteState::default();
    let mut rewritten = String::with_capacity(markdown.len());

    for line in markdown.split_inclusive('\n') {
        let (body, newline) = line
            .strip_suffix('\n')
            .map_or((line, ""), |body| (body, "\n"));
        let (body, carriage_return) = body
            .strip_suffix('\r')
            .map_or((body, ""), |body| (body, "\r"));
        let body = rewrite_markdown_line(body, &mut state, &markers);
        rewritten.push_str(&body);
        rewritten.push_str(carriage_return);
        rewritten.push_str(newline);
    }
    rewritten
}

#[derive(Default)]
struct MarkdownRewriteState {
    frontmatter: bool,
    frontmatter_complete: bool,
}

struct MarkdownRewriteMarkers {
    current_project: String,
    current_project_quoted: String,
    next_project: String,
    current_task_id: String,
    next_task_id: String,
    current_link: String,
    next_link: String,
}

impl MarkdownRewriteMarkers {
    fn new(current: &ProjectIdentity, next: &ProjectIdentity) -> Self {
        Self {
            current_project: format!("project: {}", current.title()),
            current_project_quoted: format!(
                "project: {}",
                serde_json::Value::String(current.title().to_string())
            ),
            next_project: format!(
                "project: {}",
                serde_json::Value::String(next.title().to_string())
            ),
            current_task_id: format!("id: {}-", current.id()),
            next_task_id: format!("id: {}-", next.id()),
            current_link: format!("[[{}-", current.id()),
            next_link: format!("[[{}-", next.id()),
        }
    }
}

fn rewrite_markdown_line(
    body: &str,
    state: &mut MarkdownRewriteState,
    markers: &MarkdownRewriteMarkers,
) -> String {
    if !state.frontmatter_complete && body == "---" {
        update_frontmatter_state(state);
        return body.replace(&markers.current_link, &markers.next_link);
    }
    let body = if state.frontmatter {
        rewrite_frontmatter_line(body, markers)
    } else {
        body.to_owned()
    };
    body.replace(&markers.current_link, &markers.next_link)
}

fn update_frontmatter_state(state: &mut MarkdownRewriteState) {
    if state.frontmatter {
        state.frontmatter = false;
        state.frontmatter_complete = true;
        return;
    }
    state.frontmatter = true;
}

fn rewrite_frontmatter_line(body: &str, markers: &MarkdownRewriteMarkers) -> String {
    if body == markers.current_project || body == markers.current_project_quoted {
        return markers.next_project.clone();
    }
    if body.starts_with(&markers.current_task_id) {
        return body.replacen(&markers.current_task_id, &markers.next_task_id, 1);
    }
    body.to_owned()
}

fn renamed_markdown_name(
    path: &Path,
    current: &ProjectIdentity,
    next: &ProjectIdentity,
) -> Option<OsString> {
    let file_name = path.file_name()?;
    let current_index = format!("{}.md", current.title());
    if file_name == OsStr::new(&current_index) {
        return Some(OsString::from(format!("{}.md", next.title())));
    }
    let file_name = file_name.to_str()?;
    let current_prefix = format!("{}-", current.id());
    file_name
        .strip_prefix(&current_prefix)
        .map(|suffix| OsString::from(format!("{}-{suffix}", next.id())))
}

fn sibling_name(source_name: &OsStr, suffix: &str) -> OsString {
    let mut name = OsString::from(".");
    name.push(source_name);
    name.push(suffix);
    name
}

fn reject_existing_destination(path: &Path) -> Result<(), ObsidianStoreError> {
    reject_existing(path, |path| {
        ObsidianStoreError::ProjectRenameDestinationExists { path }
    })
}

fn reject_existing_staging(path: &Path) -> Result<(), ObsidianStoreError> {
    reject_existing(path, |path| {
        ObsidianStoreError::ProjectRenameStagingExists { path }
    })
}

fn reject_existing_backup(path: &Path) -> Result<(), ObsidianStoreError> {
    reject_existing(path, |path| ObsidianStoreError::ProjectRenameBackupExists {
        path,
    })
}

fn reject_existing(
    path: &Path,
    error: impl FnOnce(PathBuf) -> ObsidianStoreError,
) -> Result<(), ObsidianStoreError> {
    match path.try_exists() {
        Ok(false) => Ok(()),
        Ok(true) => Err(error(path.to_path_buf())),
        Err(source) => Err(ObsidianStoreError::InspectProjectRenamePath {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, fs, io, path::Path};

    use pwf_models::project::{ProjectId, ProjectIdentity, ProjectName};

    use super::*;

    fn identity(id: &str, title: &str) -> ProjectIdentity {
        ProjectIdentity::new(
            ProjectId::try_new(id).unwrap(),
            ProjectName::try_new(title).unwrap(),
        )
    }

    fn write_fixture(source: &Path) {
        fs::create_dir_all(source).unwrap();
        fs::write(
            source.join("sample-app.md"),
            "---\nid: old\ntitle: sample-app\n---\n\n# Pending\n- [ ] [[OLD-0079]]\n- [x] [[OLD-NOTE-0001]]\n",
        )
        .unwrap();
        fs::write(
            source.join("OLD-0079.md"),
            "---\nid: OLD-0079\nproject: sample-app\nstatus: active\ncreated: 2026-07-01T12:00:00Z\n---\n\nKeep this body and bare OLD-0079 text unchanged.\n",
        )
        .unwrap();
        fs::write(
            source.join("OLD-NOTE-0001.md"),
            "---\nid: OLD-NOTE-0001\nproject: sample-app\nstatus: done\ncreated: 2026-06-01T12:00:00Z\ncompleted: 2026-06-02T12:00:00Z\n---\n\nCompleted body stays byte-for-byte.\n",
        )
        .unwrap();
    }

    #[test]
    fn stage_rewrites_project_identity_without_changing_task_state_or_bodies() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("sample-app");
        let destination = directory.path().join("renamed-app");
        write_fixture(&source);

        let staged = stage(
            &source,
            &destination,
            &identity("OLD", "sample-app"),
            &identity("NEW", "renamed-app"),
        )
        .unwrap();

        assert!(staged.staging_directory.join("renamed-app.md").is_file());
        assert!(staged.staging_directory.join("NEW-0079.md").is_file());
        assert!(staged.staging_directory.join("NEW-NOTE-0001.md").is_file());
        assert!(!staged.staging_directory.join("OLD-0079.md").exists());
        assert_eq!(
            fs::read_to_string(staged.staging_directory.join("renamed-app.md")).unwrap(),
            "---\nid: old\ntitle: sample-app\n---\n\n# Pending\n- [ ] [[OLD-0079]]\n- [x] [[OLD-NOTE-0001]]\n"
        );
        assert_eq!(
            fs::read_to_string(staged.staging_directory.join("NEW-0079.md")).unwrap(),
            "---\nid: NEW-0079\nproject: \"renamed-app\"\nstatus: active\ncreated: 2026-07-01T12:00:00Z\n---\n\nKeep this body and bare OLD-0079 text unchanged.\n"
        );
        assert_eq!(
            fs::read_to_string(staged.staging_directory.join("NEW-NOTE-0001.md")).unwrap(),
            "---\nid: NEW-NOTE-0001\nproject: \"renamed-app\"\nstatus: done\ncreated: 2026-06-01T12:00:00Z\ncompleted: 2026-06-02T12:00:00Z\n---\n\nCompleted body stays byte-for-byte.\n"
        );
        assert!(source.is_dir());
        assert!(!destination.exists());
    }

    #[test]
    fn rename_preserves_quoted_project_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("old");
        let destination = directory.path().join("new");
        fs::create_dir(&source).unwrap();
        fs::write(
            source.join("OLD-0001.md"),
            concat!(
                "---\n",
                "id: OLD-0001\n",
                "project: \"Web: UI; #123\"\n",
                "title: \"Keep: the title\"\n",
                "---\n\n",
                "project: Web: UI; #123\n",
            ),
        )
        .unwrap();

        ObsidianProjectTaskFilesClient
            .stage_project_rename(
                &source,
                &destination,
                &identity("OLD", "Web: UI; #123"),
                &identity("NEW", "API: \"next\"; #456"),
            )
            .unwrap()
            .commit()
            .unwrap();

        let file = MarkdownFile::open(destination.join("NEW-0001.md")).unwrap();
        let metadata = file.frontmatter::<serde_json::Value>().unwrap().unwrap();
        assert_eq!(metadata["project"], "API: \"next\"; #456");
        assert_eq!(metadata["title"], "Keep: the title");
        assert_eq!(file.body(), "\nproject: Web: UI; #123\n");
    }

    #[test]
    fn commit_restores_source_when_installing_staging_directory_fails() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("sample-app");
        let destination = directory.path().join("renamed-app");
        write_fixture(&source);
        let staged = stage(
            &source,
            &destination,
            &identity("OLD", "sample-app"),
            &identity("NEW", "renamed-app"),
        )
        .unwrap();
        fs::remove_dir_all(&staged.staging_directory).unwrap();

        staged.commit().unwrap_err();

        assert!(source.join("OLD-0079.md").is_file());
        assert!(!destination.exists());
        assert!(
            !directory
                .path()
                .join(".sample-app.pwf-rename-backup")
                .exists()
        );
    }

    #[test]
    fn backup_cleanup_failure_reports_committed_filesystem_state() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("sample-app");
        let destination = directory.path().join("renamed-app");
        write_fixture(&source);
        let staged = stage(
            &source,
            &destination,
            &identity("OLD", "sample-app"),
            &identity("NEW", "renamed-app"),
        )
        .unwrap();
        let backup = staged.backup_directory.clone();

        let outcome = staged
            .commit_with_backup_removal(|_| {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "staged backup cleanup failure",
                ))
            })
            .unwrap();

        assert_matches!(
            outcome,
            ProjectTaskFilesRenameCommit::BackupRetained { path, .. } if path == backup
        );
        assert!(!source.exists());
        assert!(destination.join("NEW-0079.md").is_file());
        assert!(backup.join("OLD-0079.md").is_file());
    }

    #[test]
    fn rename_preserves_authored_page_bytes_without_validating_frontmatter() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("sample-app");
        let destination = directory.path().join("renamed-app");
        write_fixture(&source);
        let page = b"---\ninvalid: [\n---\n[[OLD-0079]]\n\xff";
        fs::write(source.join("sample-app.md"), page).unwrap();
        fs::write(source.join("pwf-index.md"), b"\xff").unwrap();
        let staged = ObsidianProjectTaskFilesClient
            .stage_project_rename(
                &source,
                &destination,
                &identity("OLD", "sample-app"),
                &identity("NEW", "renamed-app"),
            )
            .unwrap();
        staged.commit().unwrap();
        assert_eq!(fs::read(destination.join("renamed-app.md")).unwrap(), page);
        assert_eq!(fs::read(destination.join("pwf-index.md")).unwrap(), b"\xff");
        assert!(destination.join("NEW-0079.md").is_file());
        assert!(!destination.join("sample-app.md").exists());
    }
}
