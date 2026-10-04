use std::{collections::HashMap, path::Path, sync::Arc};

use pwf_application::ports::task_vault::{
    NullablePatch, TaskDependencyRecord, TaskGraphRecord, TaskInsertion, TaskMutationError,
    TaskPatch, TaskSummaryRecord, TaskVault, TaskWriteSet,
};
use pwf_models::{
    project::Project,
    task::{TaskId, TaskStatus, TaskTimestamp},
};
use pwf_wire::{
    set_field::SetField,
    task::{RawTaskTags, TaskFilePath, TaskRecord},
};

use super::{ObsidianStore, ObsidianStoreError};
use crate::{
    file_transaction::content_revision,
    obsidian::{
        FrontmatterView, MarkdownFile, MarkdownFileError,
        identity::{TaskFile, TaskGraphFiles, TaskRead, parse_task_metadata},
        note_frontmatter::{
            activate_status, parse_blocked_by, set_blocked_by, set_commits, set_completed_at,
            set_effort, set_priority, set_status, set_tags,
        },
        note_text::replace_body,
    },
};

fn frontmatter_field(
    frontmatter: &FrontmatterView<'_>,
    key: &str,
) -> Result<Option<String>, ObsidianStoreError> {
    frontmatter
        .get(key)
        .map(|value| {
            value
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
        })
        .map_err(read_task_file_error)
}

fn task_timestamp(
    frontmatter: &FrontmatterView<'_>,
    path: &Path,
    property: &'static str,
) -> Result<Option<TaskTimestamp>, ObsidianStoreError> {
    frontmatter_field(frontmatter, property)?
        .map(|value| {
            value
                .parse()
                .map_err(|source| ObsidianStoreError::InvalidTaskTimestamp {
                    path: path.to_path_buf(),
                    property,
                    value,
                    source,
                })
        })
        .transpose()
}

pub(super) fn task_graph_metadata(
    decoded_title: Option<String>,
    file: &MarkdownFile,
    frontmatter: &FrontmatterView<'_>,
) -> Result<TaskGraphRecord, ObsidianStoreError> {
    let status = task_status(file.path(), frontmatter)?;
    let title = task_title(decoded_title, frontmatter)?;
    Ok(TaskGraphRecord {
        title,
        status,
        blocked_by: parse_blocked_by(Some(frontmatter)),
    })
}

fn task_status(
    path: &Path,
    frontmatter: &FrontmatterView<'_>,
) -> Result<TaskStatus, ObsidianStoreError> {
    frontmatter_field(frontmatter, "status")?
        .as_deref()
        .map_or(Ok(TaskStatus::Active), |value| {
            value
                .parse()
                .map_err(|source| ObsidianStoreError::InvalidTaskStatus {
                    path: path.to_path_buf(),
                    value: value.to_string(),
                    source,
                })
        })
}

fn task_title(
    decoded_title: Option<String>,
    frontmatter: &FrontmatterView<'_>,
) -> Result<String, ObsidianStoreError> {
    Ok(decoded_title
        .filter(|title| !title.trim().is_empty())
        .or(frontmatter_field(frontmatter, "title")?)
        .unwrap_or_default())
}

pub(super) fn task_summary_metadata(
    id: TaskId,
    decoded_title: Option<String>,
    file: &MarkdownFile,
    frontmatter: &FrontmatterView<'_>,
) -> Result<(TaskSummaryRecord, Option<TaskTimestamp>), ObsidianStoreError> {
    let path = file.path();
    let status = task_status(path, frontmatter)?;
    let title = task_title(decoded_title, frontmatter)?;
    let created_at = task_timestamp(frontmatter, path, "created_at")?;
    let completed_at = task_timestamp(frontmatter, path, "completed_at")?;
    Ok((
        TaskSummaryRecord {
            id,
            title,
            status,
            created_at,
            tags: frontmatter_field(frontmatter, "tags")?
                .map(|raw| RawTaskTags::new(raw.into_boxed_str().into_string())),
            effort: frontmatter_field(frontmatter, "effort")?,
            priority: frontmatter_field(frontmatter, "priority")?,
        },
        completed_at,
    ))
}

struct TaskFileMetadata {
    summary: TaskSummaryRecord,
    completed_at: Option<TaskTimestamp>,
    commits: Option<String>,
    blocked_by: pwf_wire::task::StoredBlockedBy,
}

fn task_file_metadata(
    id: TaskId,
    decoded_title: Option<String>,
    file: &MarkdownFile,
    frontmatter: &FrontmatterView<'_>,
) -> Result<TaskFileMetadata, ObsidianStoreError> {
    let (summary, completed_at) = task_summary_metadata(id, decoded_title, file, frontmatter)?;
    Ok(TaskFileMetadata {
        summary,
        completed_at,
        commits: frontmatter_field(frontmatter, "commits")?,
        blocked_by: parse_blocked_by(Some(frontmatter)),
    })
}

impl TaskFileMetadata {
    fn into_record(self, file: MarkdownFile) -> TaskRecord {
        let body = file.body().to_string();
        let revision = content_revision(file.source().as_bytes());
        let (path, source) = file.into_parts();
        TaskRecord {
            id: self.summary.id,
            title: self.summary.title,
            status: self.summary.status,
            created_at: self.summary.created_at,
            completed_at: self.completed_at,
            commits: self.commits,
            tags: self.summary.tags,
            effort: self.summary.effort,
            priority: self.summary.priority,
            blocked_by: self.blocked_by,
            body,
            source,
            locator: TaskFilePath::new(path),
            revision,
        }
    }
}

fn get_task_record(
    store: &ObsidianStore,
    project: &Project,
    id: &TaskId,
) -> Result<Option<TaskRecord>, ObsidianStoreError> {
    let task = store.task_file_for_project(project, id)?;
    if let Some(task) = task {
        let file = match MarkdownFile::read_source(&task.path) {
            Ok(file) => file,
            Err(MarkdownFileError::Read { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                return Ok(None);
            }
            Err(error) => return Err(read_task_file_error(error)),
        };
        let Some(frontmatter) = file.frontmatter_view().map_err(read_task_file_error)? else {
            return Ok(None);
        };
        let Some((current_id, title)) = parse_task_metadata(file.path(), &frontmatter)? else {
            return Ok(None);
        };
        if current_id != *id {
            return Ok(None);
        }
        let metadata = task_file_metadata(current_id, title, &file, &frontmatter)?;
        drop(frontmatter);
        return Ok(Some(metadata.into_record(file)));
    }
    Ok(None)
}

fn list_task_records(
    store: &ObsidianStore,
    project: &Project,
) -> Result<Vec<TaskRecord>, ObsidianStoreError> {
    Ok(store
        .map_task_files(project, TaskRead::Source, task_file_metadata)?
        .into_iter()
        .map(|(metadata, file)| metadata.into_record(file))
        .collect())
}

fn list_task_summaries(
    store: &ObsidianStore,
    project: &Project,
) -> Result<Vec<TaskSummaryRecord>, ObsidianStoreError> {
    let directory = store.tasks_path(project)?;
    if !directory
        .try_exists()
        .map_err(|source| ObsidianStoreError::ReadTaskFile { source })?
    {
        store.invalidate_task_index(&directory);
        return Ok(Vec::new());
    }
    if let Some(notes) = store.task_files_indexed(&directory)?
        && let Some(summaries) = notes
            .iter()
            .map(|note| note.summary.as_deref().cloned())
            .collect()
    {
        return Ok(summaries);
    }
    Ok(store
        .map_task_files(
            project,
            TaskRead::Frontmatter,
            |id, title, file, frontmatter| {
                task_summary_metadata(id, title, file, frontmatter).map(|(summary, _)| summary)
            },
        )?
        .into_iter()
        .map(|(summary, _)| summary)
        .collect())
}

impl ObsidianStore {
    pub(super) fn apply_task_patch(
        file: &mut MarkdownFile,
        patch: &TaskPatch,
    ) -> Result<(), ObsidianStoreError> {
        if let SetField::Set(title) = patch.title.as_ref() {
            file.set_property("title", title.as_ref())
                .map_err(write_task_file_error)?;
        }
        if let SetField::Set(body) = patch.body.as_ref() {
            let updated = replace_body(file, body);
            file.replace_source(updated);
        }
        // Apply commits first to keep it adjacent to completion metadata during a close.
        match &patch.commits {
            NullablePatch::Unchanged => {}
            NullablePatch::Clear => set_commits(file, None).map_err(write_task_file_error)?,
            NullablePatch::Set(commits) => {
                set_commits(file, Some(commits)).map_err(write_task_file_error)?;
            }
        }
        match patch.status.as_ref() {
            SetField::Set(TaskStatus::Active) => {
                activate_status(file).map_err(write_task_file_error)?;
            }
            SetField::Set(status) => {
                let completed_at = match &patch.completed_at {
                    NullablePatch::Set(completed_at) => Some(completed_at),
                    NullablePatch::Unchanged | NullablePatch::Clear => None,
                };
                set_status(file, *status, completed_at).map_err(write_task_file_error)?;
            }
            SetField::NoAction => match &patch.completed_at {
                NullablePatch::Unchanged => {}
                NullablePatch::Clear => {
                    set_completed_at(file, None).map_err(write_task_file_error)?;
                }
                NullablePatch::Set(completed_at) => {
                    set_completed_at(file, Some(completed_at)).map_err(write_task_file_error)?;
                }
            },
        }
        match &patch.blocked_by {
            NullablePatch::Unchanged => {}
            NullablePatch::Clear => {
                set_blocked_by(file, None).map_err(write_task_file_error)?;
            }
            NullablePatch::Set(blocked_by) => {
                set_blocked_by(file, Some(blocked_by)).map_err(write_task_file_error)?;
            }
        }
        match patch.effort {
            NullablePatch::Unchanged => {}
            NullablePatch::Clear => set_effort(file, None).map_err(write_task_file_error)?,
            NullablePatch::Set(effort) => {
                set_effort(file, Some(effort)).map_err(write_task_file_error)?;
            }
        }
        match patch.priority {
            NullablePatch::Unchanged => {}
            NullablePatch::Clear => set_priority(file, None).map_err(write_task_file_error)?,
            NullablePatch::Set(priority) => {
                set_priority(file, Some(priority)).map_err(write_task_file_error)?;
            }
        }
        match &patch.tags {
            NullablePatch::Unchanged => {}
            NullablePatch::Clear => set_tags(file, None).map_err(write_task_file_error)?,
            NullablePatch::Set(tags) => {
                set_tags(file, Some(tags)).map_err(write_task_file_error)?;
            }
        }
        Ok(())
    }
}

fn read_task_file_error(source: MarkdownFileError) -> ObsidianStoreError {
    ObsidianStoreError::ReadTaskFile {
        source: source.into_io_error(),
    }
}

fn write_task_file_error(source: MarkdownFileError) -> ObsidianStoreError {
    ObsidianStoreError::WriteTaskFile {
        source: source.into_io_error(),
    }
}

impl TaskVault for ObsidianStore {
    type Error = ObsidianStoreError;
    type GraphSnapshot = TaskGraphFiles;

    fn task_deletion(
        &self,
        project: &Project,
    ) -> Result<pwf_wire::confirmation::TaskDeletion, Self::Error> {
        let Some(vault) = project.obsidian_vault.as_ref() else {
            return Ok(pwf_wire::confirmation::TaskDeletion::HardDelete);
        };
        let resolved = pwf_application::project::runtime_path::resolve(vault.as_ref(), &self.home)
            .map_err(|source| ObsidianStoreError::TaskVaultPath { source })?;
        let deletion = pwf_wire::confirmation::TaskDeletion::MoveToTrash {
            obsidian_vault: resolved.path().to_path_buf(),
        };
        super::super::trash::require_trash_directory(&resolved.path().join(".trash"))?;
        Ok(deletion)
    }

    fn get_task_record(
        &self,
        project: &Project,
        id: &TaskId,
    ) -> Result<Option<TaskRecord>, Self::Error> {
        get_task_record(self, project, id)
    }

    fn list_task_dependencies(
        &self,
        project: &Project,
    ) -> Result<HashMap<TaskId, TaskDependencyRecord>, Self::Error> {
        let notes = self.map_task_files(
            project,
            TaskRead::Frontmatter,
            |id, _, file, frontmatter| {
                Ok((
                    id,
                    TaskDependencyRecord {
                        blocked_by: parse_blocked_by(Some(frontmatter)),
                        locator: TaskFilePath::new(file.path().to_path_buf()),
                    },
                ))
            },
        )?;
        Ok(notes
            .into_iter()
            .map(|(dependency, _)| dependency)
            .collect())
    }

    fn list_task_graph_records(
        &self,
        project: &Project,
    ) -> Result<Self::GraphSnapshot, Self::Error> {
        let directory = self.tasks_path(project)?;
        if !directory
            .try_exists()
            .map_err(|source| ObsidianStoreError::ReadTaskFile { source })?
        {
            self.invalidate_task_index(&directory);
            return Ok(TaskGraphFiles {
                files: Arc::from([]),
            });
        }
        if let Some(files) = self.task_files_indexed(&directory)? {
            return Ok(TaskGraphFiles { files });
        }
        let notes = self.map_task_files(
            project,
            TaskRead::Frontmatter,
            |id, title, file, frontmatter| {
                Ok(TaskFile {
                    id,
                    path: file.path().to_path_buf(),
                    summary: None,
                    graph: task_graph_metadata(title, file, frontmatter)
                        .map(Arc::new)
                        .map_err(Arc::new),
                })
            },
        )?;
        Ok(TaskGraphFiles {
            files: notes.into_iter().map(|(record, _)| record).collect(),
        })
    }

    fn list_task_summaries(
        &self,
        project: &Project,
    ) -> Result<Vec<TaskSummaryRecord>, Self::Error> {
        list_task_summaries(self, project)
    }

    fn list_tasks(&self, project: &Project) -> Result<Vec<TaskRecord>, Self::Error> {
        list_task_records(self, project)
    }

    fn highest_task_id(&self, project: &Project) -> Result<Option<TaskId>, Self::Error> {
        ObsidianStore::highest_task_id(self, project)
    }

    fn insert_task(&self, insertion: TaskInsertion<'_>) -> Result<(), Self::Error> {
        self.write_new_note(insertion)
    }

    fn commit_task_writes(
        &self,
        project: &Project,
        writes: TaskWriteSet,
    ) -> Result<(), TaskMutationError<Self::Error>> {
        self.commit_task_writes_impl(project, writes)
    }
}
