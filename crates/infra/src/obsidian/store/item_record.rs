use std::{fmt::Write as _, path::Path, sync::LazyLock};

use pwf_application::{
    AppDbStore, IndexPlacement, ItemPatch, Materialization, NewItem, PendingWorkItem, RecordId,
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

/// Maps a task note's raw bytes to its persisted-state record. `source` is kept
/// byte-exact (the `pwf show` contract); typed fields come off the frontmatter.
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
    // Raw, unvalidated — the application tag filter parses lazily so an
    // unfiltered read never fails on a corrupt `tags:` value (parity with the
    // legacy read path).
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

/// Materializes an index wikilink with no backing note file. Mirrors the
/// legacy read: empty body/prompt, `locator` = the note path the id should
/// occupy, and a `MissingNote` discriminant carrying the diagnostic-facing
/// (platform display form) path for the launchability issue line.
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

/// The note path an index-linked id is expected to occupy when no note exists.
fn expected_note_path(index_path: &Path, id: &str) -> std::path::PathBuf {
    index_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{id}.md"))
}

/// [`missing_note_record`] from a parsed checkbox line (single-item `get`).
fn checkbox_to_record(index_path: &Path, line: &ParsedIndexLine) -> PendingWorkItem {
    let (status, completed) = match &line.state {
        pwf_application::IndexEntryState::Open => (WorkItemStatus::Active, None),
        pwf_application::IndexEntryState::Done(date) => (
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

    /// The index placement of `id`'s open link entry, if the project index
    /// links it — same open-link recognition as the list materialization
    /// (`scan_index`), so bare `- [[ID]]` links count too.
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

    /// Index-driven list materialization, mirroring how the vault represents
    /// the collection (and how the legacy read walked it): every open wikilink
    /// entry becomes a record (note-backed or missing-note), and every legacy
    /// inline prompt line becomes an [`RecordId::Inline`] record. A note file
    /// with no index entry is not part of the listed collection. Placement
    /// (index display path + entry line) and the RAW section label ride on each
    /// record; normalization and launchability enrichment are application's.
    fn list_pending_items(
        &self,
        project: &ProjectName,
    ) -> Result<Vec<PendingWorkItem>, ObsidianStoreError> {
        if !Path::new(&self.config.notes_dir).exists() {
            return Err(ObsidianStoreError::NotesDirectoryNotFound {
                path: self.config.notes_dir.clone(),
            });
        }
        let Some((index_path, text)) = self.validated_project_index(project)? else {
            return Ok(Vec::new());
        };
        let tasks = self.task_files_for_project(project)?;
        let index_display = path_str(&index_path);
        let scan = scan_index(&text);

        let mut records = Vec::with_capacity(scan.links.len() + scan.inline.len());
        for link in &scan.links {
            let id = WorkItemId::try_new(&link.id).expect("link regex guarantees canonical id");
            let alias = link.alias.clone().unwrap_or_default();
            let mut record = match tasks.iter().find(|task| task.id == id) {
                Some(task) => {
                    let mut record = note_to_record(
                        task.id.clone(),
                        &task.path,
                        task.title.as_deref(),
                        task.markdown.clone(),
                    );
                    // The index alias backs up an empty note title (legacy
                    // title precedence: frontmatter, alias, id).
                    if record.title.trim().is_empty() {
                        record.title = alias;
                    }
                    record
                }
                None => missing_note_record(
                    id,
                    alias,
                    WorkItemStatus::Active,
                    None,
                    None,
                    &expected_note_path(&index_path, &link.id),
                ),
            };
            record.placement = Some(IndexPlacement {
                index_path: index_display.clone(),
                line: line_number(&text, link.start),
            });
            record.section = section_label_at(&text, link.start);
            records.push(record);
        }

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
        // Commits are applied before the status/completed edit so that a close
        // (`status: done` + `completed:` inserted together) leaves `commits:`
        // anchored after `created:`, not after the freshly-inserted `completed:`
        // — the byte-for-byte frontmatter order the legacy close path produced.
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

/// Flips a legacy `- [ ]` checkbox at `line` to `- [x] … ✅ <completed>`.
///
/// Pure text mechanics for the `AppDbStore<PendingWorkItem>` status patch on an
/// inline/legacy checkbox item (`patch_legacy_checkbox`); the byte-identical CLI
/// gate depends on this being the single write path for legacy closes.
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
