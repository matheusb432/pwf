use std::{fmt::Write as _, path::Path};

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
use pwf_wire::task::RawTaskTags;

use super::{
    ObsidianStore, ObsidianStoreError,
    add::NewNoteRequest,
    fs::{line_start_index, path_str, read_task_file, write_index, write_task_file},
    index_entry::{ParsedIndexLine, parse_index_lines},
};

fn date_stamp_regex() -> &'static Regex {
    regex!(r"✅\s*(\d{4}-\d{2}-\d{2})")
}
use crate::obsidian::{
    done_queue,
    note_frontmatter::{
        reopen_status_text, set_blocked_by_text, set_commits_text, set_completed_text,
        set_effort_text, set_status_text, set_tags_text,
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
    let parsed = crate::obsidian::frontmatter_text::parse(&source);
    let frontmatter = &parsed.frontmatter;
    let status = frontmatter
        .get("status")
        .and_then(|status| status.parse().ok())
        .unwrap_or(TaskStatus::Active);
    let title = decoded_title
        .filter(|title| !title.trim().is_empty())
        .map(str::to_string)
        .or_else(|| frontmatter.get("title").cloned())
        .unwrap_or_default();
    // Preserve raw tags so unfiltered reads do not fail on invalid tag syntax.
    let tags = frontmatter.get("tags").cloned().map(RawTaskTags::new);
    let field = |key: &str| {
        frontmatter
            .get(key)
            .filter(|value| !value.trim().is_empty())
            .cloned()
    };
    let date = |property: &'static str| -> Result<Option<AppDate>, ObsidianStoreError> {
        field(property)
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
    Ok(TaskRecord {
        id,
        title,
        status,
        created: date("created")?,
        completed: date("completed")?,
        commits: field("commits"),
        tags,
        effort: field("effort"),
        blocked_by: field("blocked_by"),
        section: None,
        body: parsed.body,
        source,
        locator: path_str(path),
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
        blocked_by: None,
        section,
        body: String::new(),
        source: String::new(),
        locator: path_str(expected_path),
        placement: None,
        materialization: Materialization::MissingNote {
            expected: expected_path.display().to_string(),
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

impl ObsidianStore {
    fn get_task(
        &self,
        project: &Project,
        id: &TaskId,
    ) -> Result<Option<TaskRecord>, ObsidianStoreError> {
        if let Some(task) = self
            .task_files_for_project(project)?
            .into_iter()
            .find(|task| task.id == *id)
        {
            let mut record =
                note_to_record(task.id, &task.path, task.title.as_deref(), task.markdown)?;
            if let Some((index_path, text)) = self.validated_project_index(project)?
                && let Some(line) = parse_index_lines(&index_path, &text)?
                    .into_iter()
                    .find(|line| line.id == *id)
            {
                record.section = line.section;
                if matches!(line.state, IndexEntryState::Open) {
                    record.placement = Some(IndexPlacement {
                        index_path: path_str(&index_path),
                        line: line.line_number,
                    });
                }
            }
            return Ok(Some(record));
        }
        let Some((index_path, text)) = self.validated_project_index(project)? else {
            return Ok(None);
        };
        Ok(parse_index_lines(&index_path, &text)?
            .into_iter()
            .find(|line| line.id == *id)
            .map(|line| index_entry_to_record(&index_path, &line)))
    }

    /// Lists every note-backed and index-only task record.
    ///
    /// Open index entries contribute placement. Every index entry contributes its raw section; the
    /// application owns lifecycle visibility, normalization, and launchability policy.
    fn list_tasks(&self, project: &Project) -> Result<Vec<TaskRecord>, ObsidianStoreError> {
        let project_directory = self.tasks_path(project)?;
        if !project_directory.exists() {
            return Err(ObsidianStoreError::NotesDirectoryNotFound {
                path: project_directory.display().to_string(),
            });
        }
        let tasks = self.task_files_for_project(project)?;
        let mut records: Vec<TaskRecord> = tasks
            .into_iter()
            .map(|task| note_to_record(task.id, &task.path, task.title.as_deref(), task.markdown))
            .collect::<Result<_, _>>()?;
        let Some((index_path, text)) = self.validated_project_index(project)? else {
            return Ok(records);
        };
        let index_display = path_str(&index_path);

        for line in parse_index_lines(&index_path, &text)? {
            if let Some(record) = records.iter_mut().find(|record| record.id == line.id) {
                if record.title.trim().is_empty() {
                    record.title = line.alias.clone().unwrap_or_default();
                }
                record.section.clone_from(&line.section);
                if matches!(&line.state, IndexEntryState::Open) {
                    record.placement = Some(IndexPlacement {
                        index_path: index_display.clone(),
                        line: line.line_number,
                    });
                }
                continue;
            }

            let mut record = index_entry_to_record(&index_path, &line);
            if matches!(&line.state, IndexEntryState::Open) {
                record.placement = Some(IndexPlacement {
                    index_path: index_display.clone(),
                    line: line.line_number,
                });
            }
            records.push(record);
        }

        Ok(records)
    }

    fn insert_task(
        &self,
        project: &Project,
        new: &NewTask,
    ) -> Result<TaskRecord, ObsidianStoreError> {
        let note = self.write_new_note(
            project,
            &NewNoteRequest {
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
        let mut content = read_task_file(note_path)?;
        if let Some(title) = &patch.title {
            content = replace_title(&content, title.as_ref());
        }
        if let Some(body) = &patch.body {
            content = replace_body(&content, body);
        }
        // Apply commits first to keep `commits:` anchored after `created:` during a close.
        match &patch.commits {
            NullablePatch::Unchanged => {}
            NullablePatch::Clear => content = set_commits_text(&content, None),
            NullablePatch::Set(commits) => {
                content = set_commits_text(&content, Some(commits));
            }
        }
        match patch.status {
            Some(TaskStatus::Active) => content = reopen_status_text(&content),
            Some(status) => {
                let completed = match &patch.completed {
                    NullablePatch::Set(completed) => Some(completed),
                    NullablePatch::Unchanged | NullablePatch::Clear => None,
                };
                content = set_status_text(&content, status, completed);
            }
            None => match &patch.completed {
                NullablePatch::Unchanged => {}
                NullablePatch::Clear => content = set_completed_text(&content, None),
                NullablePatch::Set(completed) => {
                    content = set_completed_text(&content, Some(completed));
                }
            },
        }
        match &patch.blocked_by {
            NullablePatch::Unchanged => {}
            NullablePatch::Clear => content = set_blocked_by_text(&content, None),
            NullablePatch::Set(blocked_by) => {
                content = set_blocked_by_text(&content, Some(blocked_by));
            }
        }
        match patch.effort {
            NullablePatch::Unchanged => {}
            NullablePatch::Clear => content = set_effort_text(&content, None),
            NullablePatch::Set(effort) => content = set_effort_text(&content, Some(effort)),
        }
        match &patch.tags {
            NullablePatch::Unchanged => {}
            NullablePatch::Clear => content = set_tags_text(&content, None),
            NullablePatch::Set(tags) => content = set_tags_text(&content, Some(tags)),
        }
        write_task_file(note_path, &content)
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
                    line.line_number,
                    completed,
                    &path_str(index_path),
                )?;
                write_index(index_path, &updated)
            }
            None => Ok(()),
        }
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

impl TaskStore for ObsidianStore {
    type Error = ObsidianStoreError;

    fn get(&self, project: &Project, id: &TaskId) -> Result<Option<TaskRecord>, Self::Error> {
        self.get_task(project, id)
    }

    fn list(&self, project: &Project) -> Result<Vec<TaskRecord>, Self::Error> {
        self.list_tasks(project)
    }

    fn insert(&self, project: &Project, new: NewTask) -> Result<TaskRecord, Self::Error> {
        self.insert_task(project, &new)
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
