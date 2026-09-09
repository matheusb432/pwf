use std::{collections::HashMap, num::NonZeroUsize, path::Path};

use pwf_application::ports::task_vault::{
    IndexEntry, IndexEntryState, IndexPlacement, Materialization, NewTask, NullablePatch,
    TaskMutationError, TaskPatch, TaskRecord, TaskSummaryRecord, TaskVault, TaskWriteSet,
};
use pwf_models::{
    project::Project,
    revision::ContentRevision,
    task::{TaskId, TaskSection, TaskStatus, TaskTimestamp},
};
use pwf_wire::{
    set_field::SetField,
    task::{RawTaskTags, TaskIndexPath, TaskNotePath},
};

use super::{
    ObsidianStore, ObsidianStoreError,
    add::NewNoteRequest,
    fs::{line_start_index, path_str, read_task_file},
    index_entry::{ParsedIndexLine, parse_index_lines},
};
use crate::{
    file_transaction::content_revision,
    obsidian::{
        FrontmatterView, MarkdownFile, MarkdownFileError, done_queue,
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
            section: None,
        },
        completed_at,
    ))
}

struct TaskNoteMetadata {
    summary: TaskSummaryRecord,
    completed_at: Option<TaskTimestamp>,
    commits: Option<String>,
    blocked_by: pwf_application::ports::task_vault::StoredBlockedBy,
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
            section: None,
            body,
            source,
            locator: TaskNotePath::new(path),
            placement: None,
            materialization: Materialization::NoteFile,
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

fn missing_note_record(
    id: TaskId,
    title: String,
    status: TaskStatus,
    completed_at: Option<TaskTimestamp>,
    section: Option<TaskSection>,
    expected_path: &Path,
    revision: ContentRevision,
) -> TaskRecord {
    TaskRecord {
        id,
        title,
        status,
        created_at: None,
        completed_at,
        commits: None,
        tags: None,
        effort: None,
        priority: None,
        blocked_by: pwf_application::ports::task_vault::StoredBlockedBy::Absent,
        section,
        body: String::new(),
        source: String::new(),
        locator: TaskNotePath::new(expected_path.to_path_buf()),
        placement: None,
        materialization: Materialization::MissingNote {
            expected: TaskNotePath::new(expected_path.to_path_buf()),
        },
        revision,
    }
}

fn expected_note_path(index_path: &Path, id: &TaskId) -> std::path::PathBuf {
    index_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{id}.md"))
}

fn index_entry_to_record(
    index_path: &Path,
    line: &ParsedIndexLine,
    revision: ContentRevision,
) -> TaskRecord {
    let (status, completed_at) = match &line.state {
        IndexEntryState::Open => (TaskStatus::Active, None),
        IndexEntryState::Done(date) => (TaskStatus::Done, *date),
    };
    let expected = expected_note_path(index_path, &line.id);
    missing_note_record(
        line.id.clone(),
        line.alias.clone().unwrap_or_default(),
        status,
        completed_at,
        line.section.clone(),
        &expected,
        revision,
    )
}

fn index_placement(index_path: &Path, line: NonZeroUsize) -> IndexPlacement {
    IndexPlacement {
        index_path: TaskIndexPath::new(index_path.to_path_buf()),
        line,
    }
}

fn open_index_placement(index_path: &Path, line: &ParsedIndexLine) -> Option<IndexPlacement> {
    matches!(line.state, IndexEntryState::Open)
        .then(|| index_placement(index_path, line.line_number))
}

fn get_task_record(
    store: &ObsidianStore,
    project: &Project,
    id: &TaskId,
) -> Result<Option<TaskRecord>, ObsidianStoreError> {
    let task = store
        .task_files_for_project(project)?
        .into_iter()
        .find(|task| task.id == *id);
    if let Some(task) = task {
        let mut record = record_from_source(task.id, &task.path, task.title, task.markdown)?;
        apply_index_metadata(store, project, id, &mut record)?;
        return Ok(Some(record));
    }
    let Some((index_path, text)) = store.validated_project_index(project)? else {
        return Ok(None);
    };
    let revision = content_revision(text.as_bytes());
    Ok(parse_index_lines(&index_path, &text)?
        .into_iter()
        .find(|line| line.id == *id)
        .map(|line| index_entry_to_record(&index_path, &line, revision)))
}

fn apply_index_metadata(
    store: &ObsidianStore,
    project: &Project,
    id: &TaskId,
    record: &mut TaskRecord,
) -> Result<(), ObsidianStoreError> {
    let Some((index_path, text)) = store.validated_project_index(project)? else {
        return Ok(());
    };
    let Some(line) = parse_index_lines(&index_path, &text)?
        .into_iter()
        .find(|line| line.id == *id)
    else {
        return Ok(());
    };
    record.section.clone_from(&line.section);
    record.placement = open_index_placement(&index_path, &line);
    Ok(())
}

fn list_task_records(
    store: &ObsidianStore,
    project: &Project,
) -> Result<Vec<TaskRecord>, ObsidianStoreError> {
    let mut records: Vec<_> = store
        .map_task_notes(project, MarkdownFile::read_source, task_note_metadata)?
        .into_iter()
        .map(|(metadata, file)| metadata.into_record(file))
        .collect();
    let Some((index_path, text)) = store.validated_project_index(project)? else {
        return Ok(records);
    };
    let revision = content_revision(text.as_bytes());
    let mut slots: HashMap<TaskId, usize> = records
        .iter()
        .enumerate()
        .map(|(slot, record)| (record.id.clone(), slot))
        .collect();
    for line in parse_index_lines(&index_path, &text)? {
        merge_index_line(&mut records, &mut slots, &index_path, &line, &revision);
    }
    Ok(records)
}

fn list_task_summaries(
    store: &ObsidianStore,
    project: &Project,
) -> Result<Vec<TaskSummaryRecord>, ObsidianStoreError> {
    let mut records: Vec<_> = store
        .map_task_notes(
            project,
            MarkdownFile::read_frontmatter_file,
            |id, title, file, frontmatter| {
                note_metadata(id, title, file, frontmatter).map(|(summary, _)| summary)
            },
        )?
        .into_iter()
        .map(|(summary, _)| summary)
        .collect();
    let Some((index_path, text)) = store.validated_project_index(project)? else {
        return Ok(records);
    };
    let mut slots: HashMap<TaskId, usize> = records
        .iter()
        .enumerate()
        .map(|(slot, record)| (record.id.clone(), slot))
        .collect();
    for line in parse_index_lines(&index_path, &text)? {
        if let Some(&slot) = slots.get(&line.id) {
            let record = &mut records[slot];
            merge_index_heading(&mut record.title, &mut record.section, &line);
        } else {
            slots.insert(line.id.clone(), records.len());
            records.push(TaskSummaryRecord {
                id: line.id,
                title: line.alias.unwrap_or_default(),
                status: match line.state {
                    IndexEntryState::Open => TaskStatus::Active,
                    IndexEntryState::Done(_) => TaskStatus::Done,
                },
                created_at: None,
                tags: None,
                effort: None,
                priority: None,
                section: line.section,
            });
        }
    }
    Ok(records)
}

fn merge_index_heading(
    title: &mut String,
    section: &mut Option<TaskSection>,
    line: &ParsedIndexLine,
) {
    if title.trim().is_empty() {
        *title = line.alias.clone().unwrap_or_default();
    }
    section.clone_from(&line.section);
}

fn merge_index_line(
    records: &mut Vec<TaskRecord>,
    slots: &mut HashMap<TaskId, usize>,
    index_path: &Path,
    line: &ParsedIndexLine,
    revision: &ContentRevision,
) {
    if let Some(&slot) = slots.get(&line.id) {
        merge_existing_index_line(&mut records[slot], index_path, line);
        return;
    }
    let mut record = index_entry_to_record(index_path, line, revision.clone());
    record.placement = open_index_placement(index_path, line);
    slots.insert(record.id.clone(), records.len());
    records.push(record);
}

fn merge_existing_index_line(record: &mut TaskRecord, index_path: &Path, line: &ParsedIndexLine) {
    merge_index_heading(&mut record.title, &mut record.section, line);
    record.placement = open_index_placement(index_path, line);
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

pub(super) fn patch_index_entry_text(
    index_path: &Path,
    text: &str,
    line: &ParsedIndexLine,
    patch: &TaskPatch,
) -> Result<String, ObsidianStoreError> {
    match patch.status.as_ref() {
        SetField::Set(TaskStatus::Active) => {
            Ok(done_queue::reopen_done_link(text, &line.id).unwrap_or_else(|| text.to_string()))
        }
        SetField::Set(TaskStatus::Done | TaskStatus::Cancelled) => {
            close_index_entry_text(text, line.line_number.get(), &path_str(index_path))
        }
        SetField::NoAction => Ok(text.to_string()),
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

    fn get_task(&self, project: &Project, id: &TaskId) -> Result<Option<TaskRecord>, Self::Error> {
        get_task_record(self, project, id)
    }

    /// Lists every note-backed and index-only task record.
    ///
    /// Open index entries contribute placement. Every index entry contributes its raw section; the
    /// application owns lifecycle visibility, normalization, and launchability policy.
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

    fn read_task_markdown(&self, locator: &TaskNotePath) -> Result<String, Self::Error> {
        read_task_file(locator.as_path())
    }

    fn list_index_entries(&self, project: &Project) -> Result<Vec<IndexEntry>, Self::Error> {
        ObsidianStore::list_index_entries(self, project)
    }

    fn upsert_index_entry(&self, project: &Project, entry: IndexEntry) -> Result<(), Self::Error> {
        ObsidianStore::upsert_index_entry(self, project, &entry)
    }

    fn commit_task_writes(
        &self,
        project: &Project,
        writes: TaskWriteSet,
    ) -> Result<(), TaskMutationError<Self::Error>> {
        self.commit_task_writes_impl(project, writes)
    }
}

/// Marks an index entry done at `line` while preserving its surrounding text.
/// This must remain the only write path to preserve byte-identical CLI output.
pub(super) fn close_index_entry_text(
    content: &str,
    line: usize,
    note: &str,
) -> Result<String, ObsidianStoreError> {
    let marker_index = line_start_index(content, line).ok_or_else(|| {
        ObsidianStoreError::ExpectedOpenTaskMarker {
            note: note.to_string(),
            line,
        }
    })?;
    let marker = content
        .get(marker_index..marker_index + 5)
        .unwrap_or_default();
    if marker != "- [ ]" {
        return Err(ObsidianStoreError::ExpectedOpenTaskMarker {
            note: note.to_string(),
            line,
        });
    }
    let line_end = content[marker_index..]
        .find(['\r', '\n'])
        .map_or(content.len(), |index| marker_index + index);
    let line_text = &content[marker_index..line_end];
    let checked_line = format!("- [x]{}", &line_text[5..]);
    Ok(format!(
        "{}{}{}",
        &content[..marker_index],
        checked_line,
        &content[line_end..]
    ))
}
