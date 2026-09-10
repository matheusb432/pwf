use std::path::Path;

use pwf_application::ports::task_vault::{
    NewTask, NullablePatch, TaskDependencyRecord, TaskMutationError, TaskPatch, TaskSummaryRecord,
    TaskVault, TaskWriteSet,
};
use pwf_models::{
    project::Project,
    task::{TaskId, TaskStatus, TaskTimestamp},
};
use pwf_wire::{
    set_field::SetField,
    task::{RawTaskTags, TaskNotePath, TaskRecord},
};

use super::{ObsidianStore, ObsidianStoreError, add::NewNoteRequest};
use crate::{
    file_transaction::content_revision,
    obsidian::{
        FrontmatterView, MarkdownFile, MarkdownFileError,
        note_frontmatter::{
            parse_blocked_by, reopen_status, set_blocked_by, set_commits, set_completed_at,
            set_effort, set_priority, set_status, set_tags,
        },
        note_text::{replace_body, replace_title},
    },
};

fn note_field(
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

fn note_timestamp(
    frontmatter: &FrontmatterView<'_>,
    path: &Path,
    property: &'static str,
) -> Result<Option<TaskTimestamp>, ObsidianStoreError> {
    note_field(frontmatter, property)?
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

fn note_metadata(
    id: TaskId,
    decoded_title: Option<String>,
    file: &MarkdownFile,
    frontmatter: &FrontmatterView<'_>,
) -> Result<(TaskSummaryRecord, Option<TaskTimestamp>), ObsidianStoreError> {
    let path = file.path();
    let status =
        note_field(frontmatter, "status")?
            .as_deref()
            .map_or(Ok(TaskStatus::Active), |value| {
                value
                    .parse()
                    .map_err(|source| ObsidianStoreError::InvalidTaskStatus {
                        path: path.to_path_buf(),
                        value: value.to_string(),
                        source,
                    })
            })?;
    let title = decoded_title
        .filter(|title| !title.trim().is_empty())
        .or(note_field(frontmatter, "title")?)
        .unwrap_or_default();
    let created_at = note_timestamp(frontmatter, path, "created_at")?;
    let completed_at = note_timestamp(frontmatter, path, "completed_at")?;
    Ok((
        TaskSummaryRecord {
            id,
            title,
            status,
            created_at,
            tags: note_field(frontmatter, "tags")?.map(RawTaskTags::new),
            effort: note_field(frontmatter, "effort")?,
            priority: note_field(frontmatter, "priority")?,
        },
        completed_at,
    ))
}

struct TaskNoteMetadata {
    summary: TaskSummaryRecord,
    completed_at: Option<TaskTimestamp>,
    commits: Option<String>,
    blocked_by: pwf_wire::task::StoredBlockedBy,
}

fn task_note_metadata(
    id: TaskId,
    decoded_title: Option<String>,
    file: &MarkdownFile,
    frontmatter: &FrontmatterView<'_>,
) -> Result<TaskNoteMetadata, ObsidianStoreError> {
    let (summary, completed_at) = note_metadata(id, decoded_title, file, frontmatter)?;
    Ok(TaskNoteMetadata {
        summary,
        completed_at,
        commits: note_field(frontmatter, "commits")?,
        blocked_by: parse_blocked_by(Some(frontmatter)),
    })
}

impl TaskNoteMetadata {
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
            locator: TaskNotePath::new(path),
            revision,
        }
    }
}

fn record_from_source(
    id: TaskId,
    path: &Path,
    title: Option<String>,
    source: String,
) -> Result<TaskRecord, ObsidianStoreError> {
    let file = MarkdownFile::from_source(path, source);
    let frontmatter = file
        .frontmatter_view()
        .map_err(read_task_file_error)?
        .ok_or_else(|| ObsidianStoreError::MissingFrontmatter {
            path: path.to_path_buf(),
            property: "id",
        })?;
    let metadata = task_note_metadata(id, title, &file, &frontmatter)?;
    drop(frontmatter);
    Ok(metadata.into_record(file))
}

fn get_task_record(
    store: &ObsidianStore,
    project: &Project,
    id: &TaskId,
) -> Result<Option<TaskRecord>, ObsidianStoreError> {
    let task = store
        .map_task_notes(
            project,
            MarkdownFile::read_frontmatter_file,
            |id, _, _, _| Ok(id),
        )?
        .into_iter()
        .find(|(candidate, _)| candidate == id);
    if let Some((_, file)) = task {
        let file = MarkdownFile::read_source(file.path()).map_err(read_task_file_error)?;
        let Some(frontmatter) = file.frontmatter_view().map_err(read_task_file_error)? else {
            return Ok(None);
        };
        let Some((current_id, title)) =
            crate::obsidian::identity::parse_task_metadata(file.path(), &frontmatter)?
        else {
            return Ok(None);
        };
        if current_id != *id {
            return Ok(None);
        }
        let metadata = task_note_metadata(current_id, title, &file, &frontmatter)?;
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
        .map_task_notes(project, MarkdownFile::read_source, task_note_metadata)?
        .into_iter()
        .map(|(metadata, file)| metadata.into_record(file))
        .collect())
}

fn list_task_summaries(
    store: &ObsidianStore,
    project: &Project,
) -> Result<Vec<TaskSummaryRecord>, ObsidianStoreError> {
    Ok(store
        .map_task_notes(
            project,
            MarkdownFile::read_frontmatter_file,
            |id, title, file, frontmatter| {
                note_metadata(id, title, file, frontmatter).map(|(summary, _)| summary)
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
            let updated = replace_title(file.source(), title.as_ref());
            file.replace_source(updated);
        }
        if let SetField::Set(body) = patch.body.as_ref() {
            let updated = replace_body(file.source(), body);
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
                reopen_status(file).map_err(write_task_file_error)?;
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

    fn get_task_dependencies(
        &self,
        project: &Project,
        id: &TaskId,
    ) -> Result<Option<TaskDependencyRecord>, Self::Error> {
        let notes = self.map_task_notes(
            project,
            MarkdownFile::read_frontmatter_file,
            |candidate, _, file, frontmatter| {
                let dependency = (candidate == *id).then(|| TaskDependencyRecord {
                    blocked_by: parse_blocked_by(Some(frontmatter)),
                    locator: TaskNotePath::new(file.path().to_path_buf()),
                });
                Ok(dependency)
            },
        )?;
        Ok(notes.into_iter().find_map(|(dependency, _)| dependency))
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

    fn next_task_id(&self, project: &Project) -> Result<TaskId, Self::Error> {
        ObsidianStore::next_task_id(self, project)
    }

    fn insert_task(
        &self,
        project: &Project,
        id: &TaskId,
        new: NewTask,
    ) -> Result<TaskRecord, Self::Error> {
        let note = self.write_new_note(
            project,
            &NewNoteRequest {
                id,
                body: &new.body,
                title: &new.title,
                created_at: &new.created_at,
                blocked_by: new.blocked_by.as_ref(),
                effort: new.effort,
                priority: new.priority,
                tags: new.tags.as_ref(),
            },
        )?;
        record_from_source(
            note.id,
            &note.path,
            Some(note.title.to_string()),
            note.content,
        )
    }

    fn commit_task_writes(
        &self,
        project: &Project,
        writes: TaskWriteSet,
    ) -> Result<(), TaskMutationError<Self::Error>> {
        self.commit_task_writes_impl(project, writes)
    }
}
