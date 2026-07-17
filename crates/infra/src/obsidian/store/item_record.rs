use std::{fmt::Write as _, path::Path, sync::LazyLock};

use pwf_application::{
    AppDbStore, IndexEntryState, IndexPlacement, ItemPatch, Materialization, NewItem,
    PendingWorkItem, RecordId,
};
use pwf_domain::pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus};
use regex::Regex;

use super::{
    ObsidianStore, ObsidianStoreError,
    add::NewNoteRequest,
    fs::{line_start_index, path_str, read_item_file, write_index, write_item_file},
    index_entry::{ParsedIndexLine, parse_index_lines},
    read_parser::{line_number, scan_index, section_label_at},
};

static DATE_STAMP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"✅\s*(\d{4}-\d{2}-\d{2})").expect("valid date stamp regex"));
use crate::obsidian::{
    done_queue,
    note_frontmatter::{
        reopen_status_text, set_commits_text, set_completed_text, set_effort_text, set_prereq_text,
        set_status_text, set_tags_text,
    },
    note_text::{replace_body, replace_title},
};

/// Maps a task note to typed frontmatter fields while preserving its byte-exact source.
fn note_to_record(
    id: WorkItemId,
    path: &Path,
    decoded_title: Option<&str>,
    source: String,
) -> PendingWorkItem {
    let parsed = pwf_core::frontmatter::parse(&source);
    let frontmatter = &parsed.frontmatter;
    let status = frontmatter
        .get("status")
        .and_then(|status| status.parse().ok())
        .unwrap_or(WorkItemStatus::Active);
    let title = decoded_title
        .filter(|title| !title.trim().is_empty())
        .map(str::to_string)
        .or_else(|| frontmatter.get("title").cloned())
        .unwrap_or_default();
    // Preserve raw tags so unfiltered reads do not fail on invalid tag syntax.
    let tags = frontmatter.get("tags").cloned();
    let field = |key: &str| {
        frontmatter
            .get(key)
            .filter(|value| !value.trim().is_empty())
            .cloned()
    };
    PendingWorkItem {
        id: RecordId::Item(id),
        title,
        status,
        created: field("created").map(Timestamp::new),
        completed: field("completed").map(Timestamp::new),
        commits: field("commits"),
        tags,
        effort: field("effort"),
        prereq: field("prereq"),
        section: None,
        body: parsed.body,
        source,
        locator: path_str(path),
        placement: None,
        materialization: Materialization::NoteFile,
    }
}

/// Materializes an index link whose note file is missing.
///
/// Body and source are empty. `locator` is the expected note path, and `MissingNote` carries its
/// platform display form for diagnostics.
fn missing_note_record(
    id: WorkItemId,
    title: String,
    status: WorkItemStatus,
    completed: Option<Timestamp>,
    section: Option<String>,
    expected_path: &Path,
) -> PendingWorkItem {
    PendingWorkItem {
        id: RecordId::Item(id),
        title,
        status,
        created: None,
        completed,
        commits: None,
        tags: None,
        effort: None,
        prereq: None,
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

fn expected_note_path(index_path: &Path, id: &str) -> std::path::PathBuf {
    index_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{id}.md"))
}

fn checkbox_to_record(index_path: &Path, line: &ParsedIndexLine) -> PendingWorkItem {
    let (status, completed) = match &line.state {
        IndexEntryState::Open => (WorkItemStatus::Active, None),
        IndexEntryState::Done(date) => (
            WorkItemStatus::Done,
            (!date.as_str().is_empty()).then(|| date.clone()),
        ),
    };
    let expected = expected_note_path(index_path, line.id.as_ref());
    missing_note_record(
        line.id.clone(),
        line.alias.clone().unwrap_or_default(),
        status,
        completed,
        (!line.section.is_empty()).then(|| line.section.clone()),
        &expected,
    )
}

impl ObsidianStore {
    fn get_pending_item(
        &self,
        project: &ProjectName,
        id: &WorkItemId,
    ) -> Result<Option<PendingWorkItem>, ObsidianStoreError> {
        if let Some(task) = self
            .task_files_for_project(project)?
            .into_iter()
            .find(|task| task.id == *id)
        {
            let mut record =
                note_to_record(task.id, &task.path, task.title.as_deref(), task.markdown);
            record.placement = self.open_entry_placement(project, id)?;
            return Ok(Some(record));
        }
        let Some((index_path, text)) = self.validated_project_index(project)? else {
            return Ok(None);
        };
        Ok(parse_index_lines(&text)
            .into_iter()
            .find(|line| line.id == *id)
            .map(|line| checkbox_to_record(&index_path, &line)))
    }

    /// Returns an open link's index placement, including bare `- [[ID]]` links.
    fn open_entry_placement(
        &self,
        project: &ProjectName,
        id: &WorkItemId,
    ) -> Result<Option<IndexPlacement>, ObsidianStoreError> {
        let Some((index_path, text)) = self.validated_project_index(project)? else {
            return Ok(None);
        };
        Ok(scan_index(&text)
            .links
            .into_iter()
            .find(|link| link.id == id.as_ref())
            .map(|link| IndexPlacement {
                index_path: path_str(&index_path),
                line: line_number(&text, link.start),
            }))
    }

    /// Lists every note-backed, index-only, and inline pending-work record.
    ///
    /// Open index entries contribute placement. Every index entry contributes its raw section; the
    /// application owns lifecycle visibility, normalization, and launchability policy.
    fn list_pending_items(
        &self,
        project: &ProjectName,
    ) -> Result<Vec<PendingWorkItem>, ObsidianStoreError> {
        if !Path::new(&self.config.notes_dir).exists() {
            return Err(ObsidianStoreError::NotesDirectoryNotFound {
                path: self.config.notes_dir.clone(),
            });
        }
        let tasks = self.task_files_for_project(project)?;
        let mut records: Vec<PendingWorkItem> = tasks
            .into_iter()
            .map(|task| note_to_record(task.id, &task.path, task.title.as_deref(), task.markdown))
            .collect();
        let Some((index_path, text)) = self.validated_project_index(project)? else {
            return Ok(records);
        };
        let index_display = path_str(&index_path);

        for line in parse_index_lines(&text) {
            if let Some(record) = records
                .iter_mut()
                .find(|record| matches!(&record.id, RecordId::Item(id) if id == &line.id))
            {
                if record.title.trim().is_empty() {
                    record.title = line.alias.clone().unwrap_or_default();
                }
                record.section = (!line.section.is_empty()).then(|| line.section.clone());
                if matches!(&line.state, IndexEntryState::Open) {
                    record.placement = Some(IndexPlacement {
                        index_path: index_display.clone(),
                        line: line.line_number,
                    });
                }
                continue;
            }

            let mut record = checkbox_to_record(&index_path, &line);
            if matches!(&line.state, IndexEntryState::Open) {
                record.placement = Some(IndexPlacement {
                    index_path: index_display.clone(),
                    line: line.line_number,
                });
            }
            records.push(record);
        }

        let scan = scan_index(&text);
        for (index, inline) in scan.inline.iter().enumerate() {
            records.push(PendingWorkItem {
                id: RecordId::Inline(index + 1),
                title: inline.session.clone(),
                status: WorkItemStatus::Active,
                created: None,
                completed: None,
                commits: None,
                tags: None,
                effort: None,
                prereq: None,
                section: section_label_at(&text, inline.start),
                body: inline.prompt.clone(),
                source: inline.prompt.clone(),
                locator: index_display.clone(),
                placement: Some(IndexPlacement {
                    index_path: index_display.clone(),
                    line: line_number(&text, inline.start),
                }),
                materialization: Materialization::InlineLegacy,
            });
        }
        Ok(records)
    }

    fn insert_pending_item(
        &self,
        project: &ProjectName,
        new: &NewItem,
    ) -> Result<PendingWorkItem, ObsidianStoreError> {
        let prefix =
            pwf_core::paths::project_key(&self.config, project.as_ref()).ok_or_else(|| {
                ObsidianStoreError::ProjectMissingPrefix {
                    project: project.as_ref().to_string(),
                }
            })?;
        let note = self.write_new_note(
            project,
            prefix,
            &NewNoteRequest {
                prompt: &new.prompt,
                title: new.title.as_deref(),
                created: new.created.as_str(),
                prereq: new.prereq.as_deref(),
                effort: new.effort,
                tags: new.tags.as_ref(),
            },
        )?;
        let id = WorkItemId::try_new(&note.id).expect("allocated id is canonical");
        Ok(note_to_record(
            id,
            &note.path,
            Some(&note.title),
            note.content,
        ))
    }

    fn update_pending_item(
        &self,
        project: &ProjectName,
        id: &WorkItemId,
        patch: &ItemPatch,
    ) -> Result<(), ObsidianStoreError> {
        if let Some(task) = self
            .task_files_for_project(project)?
            .into_iter()
            .find(|task| task.id == *id)
        {
            return Self::patch_note_file(&task.path, patch);
        }
        let Some((index_path, text)) = self.validated_project_index(project)? else {
            return Err(ObsidianStoreError::ItemNotFound {
                id: id.as_ref().to_string(),
            });
        };
        let Some(line) = parse_index_lines(&text)
            .into_iter()
            .find(|line| line.id == *id)
        else {
            return Err(ObsidianStoreError::ItemNotFound {
                id: id.as_ref().to_string(),
            });
        };
        Self::patch_legacy_checkbox(&index_path, &text, &line, patch)
    }

    fn patch_note_file(note_path: &Path, patch: &ItemPatch) -> Result<(), ObsidianStoreError> {
        let mut content = read_item_file(note_path)?;
        if let Some(title) = &patch.title {
            content = replace_title(&content, title);
        }
        if let Some(body) = &patch.body {
            content = replace_body(&content, body);
        }
        // Apply commits first to keep `commits:` anchored after `created:` during a close.
        if let Some(commits) = &patch.commits {
            content = set_commits_text(&content, commits.as_deref());
        }
        match patch.status {
            Some(WorkItemStatus::Active) => content = reopen_status_text(&content),
            Some(status) => {
                let completed = patch
                    .completed
                    .as_ref()
                    .and_then(Option::as_ref)
                    .map_or("", Timestamp::as_str);
                content = set_status_text(&content, status, completed);
            }
            None => {
                if let Some(completed) = &patch.completed {
                    content =
                        set_completed_text(&content, completed.as_ref().map(Timestamp::as_str));
                }
            }
        }
        if let Some(prereq) = &patch.prereq {
            content = set_prereq_text(&content, prereq.as_deref());
        }
        if let Some(effort) = patch.effort {
            content = set_effort_text(&content, Some(effort));
        }
        if let Some(tags) = &patch.tags {
            content = set_tags_text(&content, tags.as_ref());
        }
        write_item_file(note_path, &content)
    }

    fn patch_legacy_checkbox(
        index_path: &Path,
        text: &str,
        line: &ParsedIndexLine,
        patch: &ItemPatch,
    ) -> Result<(), ObsidianStoreError> {
        match patch.status {
            Some(WorkItemStatus::Active) => {
                if let Some(updated) = done_queue::reopen_done_link(text, line.id.as_ref()) {
                    write_index(index_path, &updated)?;
                }
                Ok(())
            }
            Some(WorkItemStatus::Done | WorkItemStatus::Cancelled) => {
                let completed = patch
                    .completed
                    .as_ref()
                    .and_then(Option::as_ref)
                    .map_or("", Timestamp::as_str);
                let updated = close_legacy_checkbox_text(
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

    fn delete_pending_item(
        &self,
        project: &ProjectName,
        id: &WorkItemId,
    ) -> Result<(), ObsidianStoreError> {
        let Some(task) = self
            .task_files_for_project(project)?
            .into_iter()
            .find(|task| task.id == *id)
        else {
            return Err(ObsidianStoreError::ItemNotFound {
                id: id.as_ref().to_string(),
            });
        };
        std::fs::remove_file(&task.path)
            .map_err(|source| ObsidianStoreError::RemoveItemFile { source })
    }
}

impl AppDbStore<PendingWorkItem> for ObsidianStore {
    type Error = ObsidianStoreError;

    fn get(
        &self,
        project: &ProjectName,
        id: &WorkItemId,
    ) -> Result<Option<PendingWorkItem>, Self::Error> {
        self.get_pending_item(project, id)
    }

    fn list(&self, project: &ProjectName) -> Result<Vec<PendingWorkItem>, Self::Error> {
        self.list_pending_items(project)
    }

    fn insert(&self, project: &ProjectName, new: NewItem) -> Result<PendingWorkItem, Self::Error> {
        self.insert_pending_item(project, &new)
    }

    fn update(
        &self,
        project: &ProjectName,
        id: &WorkItemId,
        patch: ItemPatch,
    ) -> Result<(), Self::Error> {
        self.update_pending_item(project, id, &patch)
    }

    fn delete(&self, project: &ProjectName, id: &WorkItemId) -> Result<(), Self::Error> {
        self.delete_pending_item(project, id)
    }
}

/// Marks an inline checkbox done at `line` while preserving its surrounding text.
/// The byte-identical CLI gate depends on this being the only legacy-close write path.
pub(super) fn close_legacy_checkbox_text(
    content: &str,
    line: usize,
    completed: &str,
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
    if !DATE_STAMP_RE.is_match(&checked_line) {
        let _ = write!(checked_line, " ✅ {completed}");
    }
    Ok(format!(
        "{}{}{}",
        &content[..marker_index],
        checked_line,
        &content[line_end..]
    ))
}
