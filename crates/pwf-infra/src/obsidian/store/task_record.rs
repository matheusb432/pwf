use std::{fmt::Write as _, num::NonZeroUsize, path::Path};

use lazy_regex::{Regex, regex};
use pwf_application::ports::task_record::{
    IndexEntryState, IndexPlacement, Materialization, NewTask, NullablePatch, TaskPatch,
    TaskRecord, TaskStore,
};
use pwf_models::{
    AppDate,
    project::Project,
    task::{TaskId, TaskSection, TaskStatus},
};
use pwf_wire::task::{RawTaskTags, TaskIndexPath, TaskNotePath};

use super::{
    ObsidianStore, ObsidianStoreError,
    add::NewNoteRequest,
    fs::{line_start_index, open_task_file, path_str, save_task_file, write_index},
    index_entry::{ParsedIndexLine, parse_index_lines},
};

fn date_stamp_regex() -> &'static Regex {
    regex!(r"✅\s*(\d{4}-\d{2}-\d{2})")
}
use crate::obsidian::{
    MarkdownFile, MarkdownFileError, done_queue,
    note_frontmatter::{
        parse_blocked_by, reopen_status, set_blocked_by, set_commits, set_completed, set_effort,
        set_status, set_tags,
    },
    note_text::{replace_body, replace_title},
};

/// Maps a task note to typed frontmatter fields while preserving its byte-exact source.
fn note_to_record(
    id: TaskId,
    path: &Path,
    decoded_title: Option<&str>,
    source: String,
) -> Result<TaskRecord, ObsidianStoreError> {
    let file = MarkdownFile::from_source(path.to_path_buf(), source);
    let frontmatter = file.frontmatter_view().map_err(read_task_file_error)?;
    let field = |key: &str| {
        frontmatter
            .as_ref()
            .map_or(Ok(None), |frontmatter| frontmatter.get(key))
            .map(|value| {
                value
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_string)
            })
            .map_err(read_task_file_error)
    };
    let status = field("status")?
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
        .map(str::to_string)
        .or(field("title")?)
        .unwrap_or_default();
    // Preserve raw tags so unfiltered reads do not fail on invalid tag syntax.
    let tags = field("tags")?.map(RawTaskTags::new);
    let date = |property: &'static str| -> Result<Option<AppDate>, ObsidianStoreError> {
        field(property)?
            .map(|value| {
                value
                    .parse()
                    .map_err(|source| ObsidianStoreError::InvalidTaskDate {
                        path: path.to_path_buf(),
                        property,
                        value,
                        source,
                    })
            })
            .transpose()
    };
    let created = date("created")?;
    let completed = date("completed")?;
    let commits = field("commits")?;
    let effort = field("effort")?;
    let blocked_by = parse_blocked_by(frontmatter.as_ref());
    drop(frontmatter);
    let body = file.body().to_string();
    let source = file.into_source();
    Ok(TaskRecord {
        id,
        title,
        status,
        created,
        completed,
        commits,
        tags,
        effort,
        blocked_by,
        section: None,
        body,
        source,
        locator: TaskNotePath::new(path.to_path_buf()),
        placement: None,
        materialization: Materialization::NoteFile,
    })
}

/// Materializes an index link whose note file is missing.
///
/// Body and source are empty. `locator` is the expected note path, and `MissingNote` carries its
/// platform display form for diagnostics.
fn missing_note_record(
    id: TaskId,
    title: String,
    status: TaskStatus,
    completed: Option<AppDate>,
    section: Option<TaskSection>,
    expected_path: &Path,
) -> TaskRecord {
    TaskRecord {
        id,
        title,
        status,
        created: None,
        completed,
        commits: None,
        tags: None,
        effort: None,
        blocked_by: pwf_application::ports::task_record::StoredBlockedBy::Absent,
        section,
        body: String::new(),
        source: String::new(),
        locator: TaskNotePath::new(expected_path.to_path_buf()),
        placement: None,
        materialization: Materialization::MissingNote {
            expected: TaskNotePath::new(expected_path.to_path_buf()),
        },
    }
}

fn expected_note_path(index_path: &Path, id: &TaskId) -> std::path::PathBuf {
    index_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{id}.md"))
}

fn index_entry_to_record(index_path: &Path, line: &ParsedIndexLine) -> TaskRecord {
    let (status, completed) = match &line.state {
        IndexEntryState::Open => (TaskStatus::Active, None),
        IndexEntryState::Done(date) => (TaskStatus::Done, *date),
    };
    let expected = expected_note_path(index_path, &line.id);
    missing_note_record(
        line.id.clone(),
        line.alias.clone().unwrap_or_default(),
        status,
        completed,
        line.section.clone(),
        &expected,
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
        let mut record = note_to_record(task.id, &task.path, task.title.as_deref(), task.markdown)?;
        apply_index_metadata(store, project, id, &mut record)?;
        return Ok(Some(record));
    }
    let Some((index_path, text)) = store.validated_project_index(project)? else {
        return Ok(None);
    };
    Ok(parse_index_lines(&index_path, &text)?
        .into_iter()
        .find(|line| line.id == *id)
        .map(|line| index_entry_to_record(&index_path, &line)))
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
    let tasks = store.task_files_for_project(project)?;
    let mut records: Vec<TaskRecord> = tasks
        .into_iter()
        .map(|task| note_to_record(task.id, &task.path, task.title.as_deref(), task.markdown))
        .collect::<Result<_, _>>()?;
    let Some((index_path, text)) = store.validated_project_index(project)? else {
        return Ok(records);
    };
    for line in parse_index_lines(&index_path, &text)? {
        merge_index_line(&mut records, &index_path, &line);
    }
    Ok(records)
}

fn merge_index_line(records: &mut Vec<TaskRecord>, index_path: &Path, line: &ParsedIndexLine) {
    if let Some(record) = records.iter_mut().find(|record| record.id == line.id) {
        merge_existing_index_line(record, index_path, line);
        return;
    }
    let mut record = index_entry_to_record(index_path, line);
    record.placement = open_index_placement(index_path, line);
    records.push(record);
}

fn merge_existing_index_line(record: &mut TaskRecord, index_path: &Path, line: &ParsedIndexLine) {
    if record.title.trim().is_empty() {
        record.title = line.alias.clone().unwrap_or_default();
    }
    record.section.clone_from(&line.section);
    record.placement = open_index_placement(index_path, line);
}

impl ObsidianStore {
    fn get_task(
        &self,
        project: &Project,
        id: &TaskId,
    ) -> Result<Option<TaskRecord>, ObsidianStoreError> {
        get_task_record(self, project, id)
    }

    /// Lists every note-backed and index-only task record.
    ///
    /// Open index entries contribute placement. Every index entry contributes its raw section; the
    /// application owns lifecycle visibility, normalization, and launchability policy.
    fn list_tasks(&self, project: &Project) -> Result<Vec<TaskRecord>, ObsidianStoreError> {
        list_task_records(self, project)
    }

    fn insert_task(
        &self,
        project: &Project,
        id: &TaskId,
        new: &NewTask,
    ) -> Result<TaskRecord, ObsidianStoreError> {
        let note = self.write_new_note(
            project,
            &NewNoteRequest {
                id,
                body: &new.body,
                title: &new.title,
                created: &new.created,
                blocked_by: new.blocked_by.as_ref(),
                effort: new.effort,
                tags: new.tags.as_ref(),
            },
        )?;
        note_to_record(note.id, &note.path, Some(note.title.as_ref()), note.content)
    }

    fn update_task(
        &self,
        project: &Project,
        id: &TaskId,
        patch: &TaskPatch,
    ) -> Result<(), ObsidianStoreError> {
        if let Some(task) = self
            .task_files_for_project(project)?
            .into_iter()
            .find(|task| task.id == *id)
        {
            return Self::patch_note_file(&task.path, patch);
        }
        let Some((index_path, text)) = self.validated_project_index(project)? else {
            return Err(ObsidianStoreError::TaskNotFound { id: id.clone() });
        };
        let Some(line) = parse_index_lines(&index_path, &text)?
            .into_iter()
            .find(|line| line.id == *id)
        else {
            return Err(ObsidianStoreError::TaskNotFound { id: id.clone() });
        };
        Self::patch_index_entry(&index_path, &text, &line, patch)
    }

    fn patch_note_file(note_path: &Path, patch: &TaskPatch) -> Result<(), ObsidianStoreError> {
        let mut file = open_task_file(note_path)?;
        if let Some(title) = &patch.title {
            let updated = replace_title(file.source(), title.as_ref());
            file.replace_source(updated);
        }
        if let Some(body) = &patch.body {
            let updated = replace_body(file.source(), body);
            file.replace_source(updated);
        }
        // Apply commits first to keep `commits:` anchored after `created:` during a close.
        match &patch.commits {
            NullablePatch::Unchanged => {}
            NullablePatch::Clear => set_commits(&mut file, None).map_err(write_task_file_error)?,
            NullablePatch::Set(commits) => {
                set_commits(&mut file, Some(commits)).map_err(write_task_file_error)?;
            }
        }
        match patch.status {
            Some(TaskStatus::Active) => {
                reopen_status(&mut file).map_err(write_task_file_error)?;
            }
            Some(status) => {
                let completed = match &patch.completed {
                    NullablePatch::Set(completed) => Some(completed),
                    NullablePatch::Unchanged | NullablePatch::Clear => None,
                };
                set_status(&mut file, status, completed).map_err(write_task_file_error)?;
            }
            None => match &patch.completed {
                NullablePatch::Unchanged => {}
                NullablePatch::Clear => {
                    set_completed(&mut file, None).map_err(write_task_file_error)?;
                }
                NullablePatch::Set(completed) => {
                    set_completed(&mut file, Some(completed)).map_err(write_task_file_error)?;
                }
            },
        }
        match &patch.blocked_by {
            NullablePatch::Unchanged => {}
            NullablePatch::Clear => {
                set_blocked_by(&mut file, None).map_err(write_task_file_error)?;
            }
            NullablePatch::Set(blocked_by) => {
                set_blocked_by(&mut file, Some(blocked_by)).map_err(write_task_file_error)?;
            }
        }
        match patch.effort {
            NullablePatch::Unchanged => {}
            NullablePatch::Clear => set_effort(&mut file, None).map_err(write_task_file_error)?,
            NullablePatch::Set(effort) => {
                set_effort(&mut file, Some(effort)).map_err(write_task_file_error)?;
            }
        }
        match &patch.tags {
            NullablePatch::Unchanged => {}
            NullablePatch::Clear => set_tags(&mut file, None).map_err(write_task_file_error)?,
            NullablePatch::Set(tags) => {
                set_tags(&mut file, Some(tags)).map_err(write_task_file_error)?;
            }
        }
        save_task_file(&file)
    }

    fn patch_index_entry(
        index_path: &Path,
        text: &str,
        line: &ParsedIndexLine,
        patch: &TaskPatch,
    ) -> Result<(), ObsidianStoreError> {
        patch_index_entry(index_path, text, line, patch)
    }

    fn delete_task(&self, project: &Project, id: &TaskId) -> Result<(), ObsidianStoreError> {
        let Some(task) = self
            .task_files_for_project(project)?
            .into_iter()
            .find(|task| task.id == *id)
        else {
            return Err(ObsidianStoreError::TaskNotFound { id: id.clone() });
        };
        std::fs::remove_file(&task.path)
            .map_err(|source| ObsidianStoreError::RemoveTaskFile { source })
    }
}

fn patch_index_entry(
    index_path: &Path,
    text: &str,
    line: &ParsedIndexLine,
    patch: &TaskPatch,
) -> Result<(), ObsidianStoreError> {
    match patch.status {
        Some(TaskStatus::Active) => {
            if let Some(updated) = done_queue::reopen_done_link(text, &line.id) {
                write_index(index_path, &updated)?;
            }
            Ok(())
        }
        Some(TaskStatus::Done | TaskStatus::Cancelled) => {
            let completed = match &patch.completed {
                NullablePatch::Set(completed) => Some(completed),
                NullablePatch::Unchanged | NullablePatch::Clear => None,
            };
            let updated = close_index_entry_text(
                text,
                line.line_number.get(),
                completed,
                &path_str(index_path),
            )?;
            write_index(index_path, &updated)
        }
        None => Ok(()),
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

impl TaskStore for ObsidianStore {
    type Error = ObsidianStoreError;

    fn get(&self, project: &Project, id: &TaskId) -> Result<Option<TaskRecord>, Self::Error> {
        self.get_task(project, id)
    }

    fn list(&self, project: &Project) -> Result<Vec<TaskRecord>, Self::Error> {
        self.list_tasks(project)
    }

    fn next_id(&self, project: &Project) -> Result<TaskId, Self::Error> {
        self.next_task_id(project)
    }

    fn insert(
        &self,
        project: &Project,
        id: &TaskId,
        new: NewTask,
    ) -> Result<TaskRecord, Self::Error> {
        self.insert_task(project, id, &new)
    }

    fn update(&self, project: &Project, id: &TaskId, patch: TaskPatch) -> Result<(), Self::Error> {
        self.update_task(project, id, &patch)
    }

    fn delete(&self, project: &Project, id: &TaskId) -> Result<(), Self::Error> {
        self.delete_task(project, id)
    }
}

/// Marks an index entry done at `line` while preserving its surrounding text.
/// This must remain the only write path to preserve byte-identical CLI output.
pub(super) fn close_index_entry_text(
    content: &str,
    line: usize,
    completed: Option<&AppDate>,
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
    let mut checked_line = format!("- [x]{}", &line_text[5..]);
    if !date_stamp_regex().is_match(&checked_line)
        && let Some(completed) = completed
    {
        let _ = write!(checked_line, " ✅ {completed}");
    }
    Ok(format!(
        "{}{}{}",
        &content[..marker_index],
        checked_line,
        &content[line_end..]
    ))
}
