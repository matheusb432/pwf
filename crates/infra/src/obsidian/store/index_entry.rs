use std::{collections::BTreeMap, path::Path, sync::LazyLock};

use pwf_application::ports::{
    app_record::AppRecordStore,
    pending_work_record::{IndexEntry, IndexEntryState, IndexSection},
};
use pwf_models::pending_work::{ProjectName, Timestamp, WorkItemId};
use regex::Regex;

use super::{
    ObsidianStore, ObsidianStoreError,
    fs::{line_start_index, read_index, write_add_index_file, write_index},
};
use crate::obsidian::{
    identity::{
        new_project_index_content, parse_project_index_identity, validate_project_index_identity,
    },
    index_text::{
        KnownSection, add_link_to_index, add_section_block, remove_index_link, section_exists,
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

/// Contains one parsed index checkbox and its raw enclosing section label.
pub(super) struct ParsedIndexLine {
    pub id: WorkItemId,
    pub alias: Option<String>,
    pub state: IndexEntryState,
    /// Retains the raw section label without canonicalization.
    pub section: String,
    /// Uses a one-based line number within the index.
    pub line_number: usize,
}

/// Parses item checkbox lines with their raw enclosing H2 labels.
///
/// This representation mapping rejects duplicate task identities and applies no cap, eviction, or
/// normalization policy.
pub(super) fn parse_index_lines(
    index_path: &Path,
    text: &str,
) -> Result<Vec<ParsedIndexLine>, ObsidianStoreError> {
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
        // A bare `- [[ID]]` link is open, like an unchecked checkbox.
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
    let mut line_numbers_by_id = BTreeMap::<WorkItemId, Vec<usize>>::new();
    for line in &lines {
        line_numbers_by_id
            .entry(line.id.clone())
            .or_default()
            .push(line.line_number);
    }
    if let Some((id, line_numbers)) = line_numbers_by_id
        .into_iter()
        .find(|(_, line_numbers)| line_numbers.len() > 1)
    {
        return Err(ObsidianStoreError::ProjectIndexTaskIdDuplicate {
            path: index_path.to_path_buf(),
            id: id.to_string(),
            lines: line_numbers,
        });
    }
    Ok(lines)
}

/// Returns raw H2 labels in document order using the entry parser's header rules.
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

/// Rewrites matching H2 labels while preserving all other bytes and the trailing newline.
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
        let Some((index_path, text)) = self.validated_project_index(project)? else {
            return Ok(Vec::new());
        };
        Ok(parse_index_lines(&index_path, &text)?
            .into_iter()
            .map(|line| IndexEntry {
                id: line.id,
                state: line.state,
                section: line.section,
            })
            .collect())
    }

    /// Replaces an existing entry or inserts it through section placement rules.
    ///
    /// A missing index is created from the identity template. Add failures retain the
    /// created-section diagnostic payload.
    fn upsert_index_entry(
        &self,
        project: &ProjectName,
        entry: &IndexEntry,
    ) -> Result<(), ObsidianStoreError> {
        let index_path = self.project_paths.project_index_path(project)?;
        let index_dir = index_path.parent().unwrap_or(Path::new("."));
        if !index_dir.exists() {
            std::fs::create_dir_all(index_dir)
                .map_err(|source| ObsidianStoreError::CreateIndexDir { source })?;
        }
        let identity = self.project_paths.project_identity(project)?;
        let content = if index_path.is_file() {
            let content = read_index(&index_path)?;
            let actual = parse_project_index_identity(&index_path, &content)?;
            validate_project_index_identity(&index_path, &actual, identity)?;
            content
        } else {
            new_project_index_content(identity)
        };
        let new_line = render_entry_line(entry);
        if let Some(existing) = parse_index_lines(&index_path, &content)?
            .into_iter()
            .find(|line| line.id == entry.id)
        {
            let updated = replace_line(&content, existing.line_number, &new_line);
            return write_index(&index_path, &updated);
        }
        let (updated, created_section) = match KnownSection::parse(&entry.section) {
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

    /// Renames an H2 section label in place without applying application policy.
    fn rename_section_header(
        &self,
        project: &ProjectName,
        from: &str,
        to: &str,
    ) -> Result<(), ObsidianStoreError> {
        let index_path = self.project_paths.project_index_path(project)?;
        let content = read_index(&index_path)?;
        write_index(&index_path, &rename_header_lines(&content, from, to))
    }

    fn delete_index_entry(
        &self,
        project: &ProjectName,
        id: &WorkItemId,
    ) -> Result<(), ObsidianStoreError> {
        let index_path = self.project_paths.project_index_path(project)?;
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

impl AppRecordStore<IndexEntry> for ObsidianStore {
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

/// Lists and renames index sections.
///
/// [`IndexEntry`] upserts create sections implicitly. Direct insertion and deletion return
/// [`ObsidianStoreError::IndexSectionWriteUnsupported`].
impl AppRecordStore<IndexSection> for ObsidianStore {
    type Error = ObsidianStoreError;

    fn get(
        &self,
        project: &ProjectName,
        label: &String,
    ) -> Result<Option<IndexSection>, Self::Error> {
        Ok(<Self as AppRecordStore<IndexSection>>::list(self, project)?
            .into_iter()
            .find(|section| section.label == *label))
    }

    fn list(&self, project: &ProjectName) -> Result<Vec<IndexSection>, Self::Error> {
        let Some((_, text)) = self.validated_project_index(project)? else {
            return Ok(Vec::new());
        };
        Ok(parse_section_labels(&text)
            .into_iter()
            .map(|label| IndexSection { label })
            .collect())
    }

    fn insert(
        &self,
        _project: &ProjectName,
        _new: IndexSection,
    ) -> Result<IndexSection, Self::Error> {
        Err(ObsidianStoreError::IndexSectionWriteUnsupported { op: "insert" })
    }

    fn update(
        &self,
        project: &ProjectName,
        label: &String,
        patch: IndexSection,
    ) -> Result<(), Self::Error> {
        self.rename_section_header(project, label, &patch.label)
    }

    fn delete(&self, _project: &ProjectName, _label: &String) -> Result<(), Self::Error> {
        Err(ObsidianStoreError::IndexSectionWriteUnsupported { op: "delete" })
    }
}

#[cfg(test)]
mod tests {
    use pwf_application::ports::pending_work_record::{IndexEntry, IndexEntryState};
    use pwf_models::pending_work::{Timestamp, WorkItemId};

    use super::{render_entry_line, replace_line};

    #[test]
    fn done_entry_replaces_only_its_index_line() {
        let entry = IndexEntry {
            id: WorkItemId::try_new("PWF-0001").unwrap(),
            state: IndexEntryState::Done(Timestamp::new("2026-07-29")),
            section: String::new(),
        };
        let index = "# pwf\n\n- [ ] [[PWF-0001]]\n- [ ] [[PWF-0002]]\n";

        assert_eq!(
            replace_line(index, 3, &render_entry_line(&entry)),
            "# pwf\n\n- [x] [[PWF-0001]] ✅ 2026-07-29\n- [ ] [[PWF-0002]]\n"
        );
    }
}
