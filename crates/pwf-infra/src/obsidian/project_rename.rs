use std::{
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
};

use pwf_application::ports::project_task_files::{
    ProjectTaskFilesClient, ProjectTaskFilesRenameCommit, StagedProjectTaskFilesRename,
};
use pwf_models::project::ProjectIndexIdentity;
use walkdir::WalkDir;

use super::{ObsidianStoreError, identity::project_index_frontmatter_id};

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
        current: &ProjectIndexIdentity,
        next: &ProjectIndexIdentity,
    ) -> Result<Self::StagedRename, Self::Error> {
        stage(source, destination, current, next)
    }
}

/// Copies and rewrites one project directory without changing the source.
fn stage(
    source: &Path,
    destination: &Path,
    current: &ProjectIndexIdentity,
    next: &ProjectIndexIdentity,
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
        reject_existing_destination(&self.destination)?;
        reject_existing_backup(&self.backup_directory)?;
        if let Err(source_error) = fs::rename(&self.source, &self.backup_directory) {
            let _ = fs::remove_dir_all(&self.staging_directory);
            return Err(ObsidianStoreError::MoveProjectRenameSource {
                from: self.source,
                to: self.backup_directory,
                source: source_error,
            });
        }
        if let Err(install_source) = fs::rename(&self.staging_directory, &self.destination) {
            return match fs::rename(&self.backup_directory, &self.source) {
                Ok(()) => match fs::remove_dir_all(&self.staging_directory) {
                    Ok(()) => Err(ObsidianStoreError::InstallStagedProjectRename {
                        from: self.staging_directory,
                        to: self.destination,
                        source: install_source,
                    }),
                    Err(cleanup_source) => {
                        Err(ObsidianStoreError::RemoveRestoredProjectRenameStaging {
                            path: self.staging_directory,
                            from: self.source,
                            to: self.destination,
                            install_source,
                            cleanup_source,
                        })
                    }
                },
                Err(restore_source) => Err(ObsidianStoreError::RestoreProjectRenameSource {
                    source_path: self.source,
                    from: self.staging_directory,
                    to: self.destination,
                    install_source,
                    restore_source,
                }),
            };
        }
        match remove_backup(&self.backup_directory) {
            Ok(()) => Ok(ProjectTaskFilesRenameCommit::Complete),
            Err(source) => Ok(ProjectTaskFilesRenameCommit::BackupRetained {
                path: self.backup_directory,
                source: Box::new(source),
            }),
        }
    }
}

fn copy_and_rewrite(
    staged: &StagedProjectRename,
    current: &ProjectIndexIdentity,
    next: &ProjectIndexIdentity,
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
            fs::create_dir(&destination).map_err(|source| {
                ObsidianStoreError::CreateStagedProjectRenameDirectory {
                    path: destination,
                    source,
                }
            })?;
        } else if entry.file_type().is_file() {
            fs::copy(entry.path(), &destination).map_err(|source| {
                ObsidianStoreError::CopyProjectRenameEntry {
                    from: entry.path().to_path_buf(),
                    to: destination,
                    source,
                }
            })?;
        } else {
            return Err(ObsidianStoreError::ProjectRenameEntryUnsupported {
                path: entry.path().to_path_buf(),
            });
        }
    }
    Ok(())
}

fn rewrite_markdown(
    staging_directory: &Path,
    current: &ProjectIndexIdentity,
    next: &ProjectIndexIdentity,
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
        if let Some(name) = renamed_markdown_name(path, current, next) {
            let destination = path.with_file_name(name);
            if destination != *path && destination.exists() {
                return Err(ObsidianStoreError::ProjectRenameFileExists { path: destination });
            }
            renames.push((path.clone(), destination));
        }
    }
    for path in markdown_paths {
        rewrite_markdown_file(&path, current, next)?;
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

fn rewrite_markdown_file(
    path: &Path,
    current: &ProjectIndexIdentity,
    next: &ProjectIndexIdentity,
) -> Result<(), ObsidianStoreError> {
    let bytes =
        fs::read(path).map_err(|source| ObsidianStoreError::ReadStagedProjectRenameFile {
            path: path.to_path_buf(),
            source,
        })?;
    let markdown = String::from_utf8(bytes).map_err(|_| {
        ObsidianStoreError::StagedProjectRenameFileNotUtf8 {
            path: path.to_path_buf(),
        }
    })?;
    let rewritten = rewrite_markdown_text(&markdown, current, next);
    if rewritten != markdown {
        fs::write(path, rewritten).map_err(|source| {
            ObsidianStoreError::WriteStagedProjectRenameFile {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    Ok(())
}

fn rewrite_markdown_text(
    markdown: &str,
    current: &ProjectIndexIdentity,
    next: &ProjectIndexIdentity,
) -> String {
    let current_task_id = format!("id: {}-", current.id());
    let next_task_id = format!("id: {}-", next.id());
    let current_link = format!("[[{}-", current.id());
    let next_link = format!("[[{}-", next.id());
    let mut frontmatter = false;
    let mut frontmatter_complete = false;
    let mut rewritten = String::with_capacity(markdown.len());

    for line in markdown.split_inclusive('\n') {
        let (body, newline) = line
            .strip_suffix('\n')
            .map_or((line, ""), |body| (body, "\n"));
        let (body, carriage_return) = body
            .strip_suffix('\r')
            .map_or((body, ""), |body| (body, "\r"));
        let mut body = body.to_string();
        if !frontmatter_complete && body == "---" {
            if frontmatter {
                frontmatter = false;
                frontmatter_complete = true;
            } else {
                frontmatter = true;
            }
        } else if frontmatter {
            if body == format!("id: {}", project_index_frontmatter_id(current)) {
                body = format!("id: {}", project_index_frontmatter_id(next));
            } else if body == format!("title: {}", current.title()) {
                body = format!("title: {}", next.title());
            } else if body == format!("project: {}", current.title()) {
                body = format!("project: {}", next.title());
            } else if body.starts_with(&current_task_id) {
                body = body.replacen(&current_task_id, &next_task_id, 1);
            }
        }
        body = body.replace(&current_link, &next_link);
        rewritten.push_str(&body);
        rewritten.push_str(carriage_return);
        rewritten.push_str(newline);
    }
    rewritten
}

fn renamed_markdown_name(
    path: &Path,
    current: &ProjectIndexIdentity,
    next: &ProjectIndexIdentity,
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

    use pwf_models::project::{ProjectId, ProjectIndexIdentity, ProjectName};

    use super::*;

    fn identity(id: &str, title: &str) -> ProjectIndexIdentity {
        ProjectIndexIdentity::new(
            ProjectId::try_new(id).unwrap(),
            ProjectName::try_new(title).unwrap(),
        )
    }

    fn write_fixture(source: &Path) {
        fs::create_dir_all(source).unwrap();
        fs::write(
            source.join("ssh-agent-phone-app.md"),
            "---\nid: ssh\ntitle: ssh-agent-phone-app\n---\n\n# Pending\n- [ ] [[SSH-0079]]\n- [x] [[SSH-NOTE-0001]]\n",
        )
        .unwrap();
        fs::write(
            source.join("SSH-0079.md"),
            "---\nid: SSH-0079\nproject: ssh-agent-phone-app\nstatus: active\ncreated: 2026-07-01T12:00:00Z\n---\n\nKeep this body and bare SSH-0079 text unchanged.\n",
        )
        .unwrap();
        fs::write(
            source.join("SSH-NOTE-0001.md"),
            "---\nid: SSH-NOTE-0001\nproject: ssh-agent-phone-app\nstatus: done\ncreated: 2026-06-01T12:00:00Z\ncompleted: 2026-06-02T12:00:00Z\n---\n\nCompleted body stays byte-for-byte.\n",
        )
        .unwrap();
    }

    #[test]
    fn stage_rewrites_project_identity_without_changing_task_state_or_bodies() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("ssh-agent-phone-app");
        let destination = directory.path().join("mimux");
        write_fixture(&source);

        let staged = stage(
            &source,
            &destination,
            &identity("SSH", "ssh-agent-phone-app"),
            &identity("MUX", "mimux"),
        )
        .unwrap();

        assert!(staged.staging_directory.join("mimux.md").is_file());
        assert!(staged.staging_directory.join("MUX-0079.md").is_file());
        assert!(staged.staging_directory.join("MUX-NOTE-0001.md").is_file());
        assert!(!staged.staging_directory.join("SSH-0079.md").exists());
        assert_eq!(
            fs::read_to_string(staged.staging_directory.join("mimux.md")).unwrap(),
            "---\nid: mux\ntitle: mimux\n---\n\n# Pending\n- [ ] [[MUX-0079]]\n- [x] [[MUX-NOTE-0001]]\n"
        );
        assert_eq!(
            fs::read_to_string(staged.staging_directory.join("MUX-0079.md")).unwrap(),
            "---\nid: MUX-0079\nproject: mimux\nstatus: active\ncreated: 2026-07-01T12:00:00Z\n---\n\nKeep this body and bare SSH-0079 text unchanged.\n"
        );
        assert_eq!(
            fs::read_to_string(staged.staging_directory.join("MUX-NOTE-0001.md")).unwrap(),
            "---\nid: MUX-NOTE-0001\nproject: mimux\nstatus: done\ncreated: 2026-06-01T12:00:00Z\ncompleted: 2026-06-02T12:00:00Z\n---\n\nCompleted body stays byte-for-byte.\n"
        );
        assert!(source.is_dir());
        assert!(!destination.exists());
    }

    #[test]
    fn commit_restores_source_when_installing_staging_directory_fails() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("ssh-agent-phone-app");
        let destination = directory.path().join("mimux");
        write_fixture(&source);
        let staged = stage(
            &source,
            &destination,
            &identity("SSH", "ssh-agent-phone-app"),
            &identity("MUX", "mimux"),
        )
        .unwrap();
        fs::remove_dir_all(&staged.staging_directory).unwrap();

        staged.commit().unwrap_err();

        assert!(source.join("SSH-0079.md").is_file());
        assert!(!destination.exists());
        assert!(
            !directory
                .path()
                .join(".ssh-agent-phone-app.pwf-rename-backup")
                .exists()
        );
    }

    #[test]
    fn backup_cleanup_failure_reports_committed_filesystem_state() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("ssh-agent-phone-app");
        let destination = directory.path().join("mimux");
        write_fixture(&source);
        let staged = stage(
            &source,
            &destination,
            &identity("SSH", "ssh-agent-phone-app"),
            &identity("MUX", "mimux"),
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
        assert!(destination.join("MUX-0079.md").is_file());
        assert!(backup.join("SSH-0079.md").is_file());
    }
}
