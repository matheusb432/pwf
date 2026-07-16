use std::{path::Path, sync::LazyLock};

use pwf_application::{
    AppDbStore, IndexEntry, IndexEntryState, IndexSection as IndexSectionRecord,
};
use pwf_domain::pending_work::{ProjectName, Timestamp, WorkItemId};
use regex::Regex;

use super::{
    ObsidianStore, ObsidianStoreError,
    fs::{line_start_index, read_index, write_add_index_file, write_index},
};
use crate::obsidian::{
    identity::{
        configured_project_index_identity, new_project_index_content, parse_project_index_identity,
        validate_project_index_identity,
    },
    index_text::{
        IndexSection, add_link_to_index, add_section_block, remove_index_link, section_exists,
    },
};

static HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^##\s+(?P<label>.+?)\s*$").expect("valid header regex"));
static TASK_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\s*-\s*(?:\[(?P<mark>[ xX])\]\s*)?\[\[(?P<id>[A-Z]{2,4}-\d{4})(?:\|(?P<alias>[^\]]+))?\]\]",
    )
    .expect("valid task-line regex")
});
static DATE_STAMP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"✅\s*(\d{4}-\d{2}-\d{2})").expect("valid date stamp regex"));

/// One checkbox line resolved from a project index, carrying the raw text and
/// section label needed by both the [`IndexEntry`] and legacy-checkbox
/// [`super::item_record`] mappings.
pub(super) struct ParsedIndexLine {
    pub id: WorkItemId,
    pub alias: Option<String>,
    pub state: IndexEntryState,
    /// Raw stored section label (e.g. "Futuro"), never canonicalized.
    pub section: String,
    /// 1-based line number within the index.
    pub line_number: usize,
}

/// Parses every `- [ ] [[ID]]` / `- [x] [[ID]] ✅ <date>` checkbox line from an
/// index, tagging each with the raw label of its enclosing `## <label>` header.
/// Pure representation mapping — no cap, eviction, or normalization policy.
pub(super) fn parse_index_lines(text: &str) -> Vec<ParsedIndexLine> {
    let mut section = String::new();
    let mut lines = Vec::new();
    for (index, raw) in text.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(header) = HEADER_RE.captures(line) {
            section = header["label"].trim().to_string();
            continue;
        }
        let Some(task) = TASK_LINE_RE.captures(line) else {
            continue;
        };
        let id = WorkItemId::try_new(&task["id"]).expect("regex guarantees canonical id");
        let alias = task.name("alias").map(|alias| alias.as_str().to_string());
        // A missing mark group is a bare `- [[ID]]` wikilink — an open entry,
        // same as an unchecked `- [ ]` (mirroring the item-list `scan_index`
        // vocabulary and the legacy `mark_done` open-link recognition).
        let is_done = matches!(task.name("mark").map(|m| m.as_str()), Some("x" | "X"));
        let state = if is_done {
            let date = DATE_STAMP_RE
                .captures(line)
                .map(|captures| captures[1].to_string())
                .unwrap_or_default();
            IndexEntryState::Done(Timestamp::new(date))
        } else {
            IndexEntryState::Open
        };
        lines.push(ParsedIndexLine {
            id,
            alias,
            state,
            section: section.clone(),
            line_number: index + 1,
        });
    }
    lines
}

/// Every H2 header's raw label in document order — the section regions the
/// [`IndexSectionRecord`] kind reports. Same header recognition as
/// [`parse_index_lines`], so entry sectioning and section listing cannot drift.
pub(super) fn parse_section_labels(text: &str) -> Vec<String> {
    text.split('\n')
        .filter_map(|raw| {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            HEADER_RE
                .captures(line)
                .map(|header| header["label"].trim().to_string())
        })
        .collect()
}

fn render_entry_line(entry: &IndexEntry) -> String {
    match &entry.state {
        IndexEntryState::Open => format!("- [ ] [[{}]]", entry.id.as_ref()),
        IndexEntryState::Done(date) => {
            format!("- [x] [[{}]] ✅ {}", entry.id.as_ref(), date.as_str())
        }
    }
}

/// Rewrites every `## <from>` header line to `## <to>`, matching the label the
/// way [`parse_section_labels`] parses it (trimmed H2 label). Preserves all
/// other lines and the trailing newline verbatim — the byte-for-byte equivalent
/// of the legacy `mark_done` futuro-rename loop.
fn rename_header_lines(content: &str, from: &str, to: &str) -> String {
    content
        .split('\n')
        .map(|line| {
            let stripped = line.strip_suffix('\r').unwrap_or(line);
            match HEADER_RE.captures(stripped) {
                Some(header) if header["label"].trim() == from => format!("## {to}"),
                _ => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn replace_line(content: &str, line_number: usize, new_line: &str) -> String {
    let Some(start) = line_start_index(content, line_number) else {
        return content.to_string();
    };
    let end = content[start..]
        .find(['\r', '\n'])
        .map_or(content.len(), |offset| start + offset);
    format!("{}{new_line}{}", &content[..start], &content[end..])
}

impl ObsidianStore {
    fn list_index_entries(
        &self,
        project: &ProjectName,
    ) -> Result<Vec<IndexEntry>, ObsidianStoreError> {
        let Some((_, text)) = self.validated_project_index(project)? else {
            return Ok(Vec::new());
        };
        Ok(parse_index_lines(&text)
            .into_iter()
            .map(|line| IndexEntry {
                id: line.id,
                state: line.state,
                section: line.section,
            })
            .collect())
    }

    /// Upsert `entry`'s index line. An existing line is replaced in place; a
    /// new line is added through the same section placement the legacy add
    /// used (`add_section_block` / `add_link_to_index`), creating the index
    /// from the identity template when it does not exist yet. The add branch
    /// writes via [`write_add_index_file`] so a failure carries the same
    /// `Failed to write index file` text + created-section diagnostic payload
    /// as the legacy add path.
    fn upsert_index_entry(
        &self,
        project: &ProjectName,
        entry: &IndexEntry,
    ) -> Result<(), ObsidianStoreError> {
        let index_path = pwf_core::paths::project_index_path(
            self.config.notes_dir_for(project.as_ref()),
            project.as_ref(),
        );
        let index_dir = index_path.parent().unwrap_or(Path::new("."));
        if !index_dir.exists() {
            std::fs::create_dir_all(index_dir)
                .map_err(|source| ObsidianStoreError::CreateIndexDir { source })?;
        }
        let identity = configured_project_index_identity(&self.config, project)?;
        let content = if index_path.is_file() {
            let content = read_index(&index_path)?;
            let actual = parse_project_index_identity(&index_path, &content)?;
            validate_project_index_identity(&index_path, &actual, &identity)?;
            content
        } else {
            new_project_index_content(&identity)
        };
        let new_line = render_entry_line(entry);
        if let Some(existing) = parse_index_lines(&content)
            .into_iter()
            .find(|line| line.id == entry.id)
        {
            let updated = replace_line(&content, existing.line_number, &new_line);
            return write_index(&index_path, &updated);
        }
        let (updated, created_section) = match IndexSection::parse(&entry.section) {
            Some(section) => (
                add_section_block(&content, &format!("{new_line}\n"), section),
                (!section_exists(&content, section)).then(|| entry.section.clone()),
            ),
            None => (add_link_to_index(&content, &new_line), None),
        };
        write_add_index_file(
            &index_path,
            &updated,
            project.as_ref(),
            created_section.as_deref(),
        )
    }

    /// Renames the `## <from>` header region to `## <to>` in place — the
    /// section-label `update` seam application uses to normalize a legacy
    /// `## Futuro` header on close (PWF-0123). Representation-only: it rewrites
    /// the matching header line, the exact textual rename the legacy `mark_done`
    /// performed.
    fn rename_section_header(
        &self,
        project: &ProjectName,
        from: &str,
        to: &str,
    ) -> Result<(), ObsidianStoreError> {
        let index_path = pwf_core::paths::project_index_path(
            self.config.notes_dir_for(project.as_ref()),
            project.as_ref(),
        );
        let content = read_index(&index_path)?;
        write_index(&index_path, &rename_header_lines(&content, from, to))
    }

    fn delete_index_entry(
        &self,
        project: &ProjectName,
        id: &WorkItemId,
    ) -> Result<(), ObsidianStoreError> {
        let index_path = pwf_core::paths::project_index_path(
            self.config.notes_dir_for(project.as_ref()),
            project.as_ref(),
        );
        let content = read_index(&index_path)?;
        let updated = remove_index_link(&content, id.as_ref());
        if updated == content {
            return Err(ObsidianStoreError::IndexLinkNotFound {
                id: id.as_ref().to_string(),
            });
        }
        write_index(&index_path, &updated)
    }
}

impl AppDbStore<IndexEntry> for ObsidianStore {
    type Error = ObsidianStoreError;

    fn get(
        &self,
        project: &ProjectName,
        id: &WorkItemId,
    ) -> Result<Option<IndexEntry>, Self::Error> {
        Ok(self
            .list_index_entries(project)?
            .into_iter()
            .find(|entry| entry.id == *id))
    }

    fn list(&self, project: &ProjectName) -> Result<Vec<IndexEntry>, Self::Error> {
        self.list_index_entries(project)
    }

    fn insert(&self, project: &ProjectName, new: IndexEntry) -> Result<IndexEntry, Self::Error> {
        self.upsert_index_entry(project, &new)?;
        Ok(new)
    }

    fn update(
        &self,
        project: &ProjectName,
        _id: &WorkItemId,
        patch: IndexEntry,
    ) -> Result<(), Self::Error> {
        self.upsert_index_entry(project, &patch)
    }

    fn delete(&self, project: &ProjectName, id: &WorkItemId) -> Result<(), Self::Error> {
        self.delete_index_entry(project, id)
    }
}

/// Mostly list-only: sections are created implicitly when an [`IndexEntry`]
/// upsert targets a missing section, so `insert`/`delete` reject with
/// [`ObsidianStoreError::IndexSectionWriteUnsupported`]. The one supported write
/// is `update`, a representation-only label rename (the futuro-normalization
/// seam, PWF-0123); reporting which regions exist is the rest of this impl's job.
impl AppDbStore<IndexSectionRecord> for ObsidianStore {
    type Error = ObsidianStoreError;

    fn get(
        &self,
        project: &ProjectName,
        label: &String,
    ) -> Result<Option<IndexSectionRecord>, Self::Error> {
        Ok(
            <Self as AppDbStore<IndexSectionRecord>>::list(self, project)?
                .into_iter()
                .find(|section| section.label == *label),
        )
    }

    fn list(&self, project: &ProjectName) -> Result<Vec<IndexSectionRecord>, Self::Error> {
        let Some((_, text)) = self.validated_project_index(project)? else {
            return Ok(Vec::new());
        };
        Ok(parse_section_labels(&text)
            .into_iter()
            .map(|label| IndexSectionRecord { label })
            .collect())
    }

    fn insert(
        &self,
        _project: &ProjectName,
        _new: IndexSectionRecord,
    ) -> Result<IndexSectionRecord, Self::Error> {
        Err(ObsidianStoreError::IndexSectionWriteUnsupported { op: "insert" })
    }

    fn update(
        &self,
        project: &ProjectName,
        label: &String,
        patch: IndexSectionRecord,
    ) -> Result<(), Self::Error> {
        self.rename_section_header(project, label, &patch.label)
    }

    fn delete(&self, _project: &ProjectName, _label: &String) -> Result<(), Self::Error> {
        Err(ObsidianStoreError::IndexSectionWriteUnsupported { op: "delete" })
    }
}
