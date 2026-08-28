use std::{collections::BTreeMap, num::NonZeroUsize, path::Path};

use lazy_regex::{Regex, regex};
use pwf_application::ports::task_record::{
    IndexEntry, IndexEntryState, IndexEntryStore, IndexSectionStore,
};
use pwf_models::{
    AppDate,
    project::Project,
    task::{TaskId, TaskSection},
};

use super::{
    ObsidianStore, ObsidianStoreError,
    fs::{line_start_index, read_index, write_add_index_file, write_index},
};
use crate::obsidian::{
    MarkdownFile,
    identity::{
        new_project_index_content, parse_project_index_identity, validate_project_index_identity,
    },
    index_text::{
        KnownSection, add_link_to_index, add_section_block, remove_index_link, section_exists,
    },
};

fn header_regex() -> &'static Regex {
    regex!(r"^##\s+(?P<label>.+?)\s*$")
}

fn task_line_regex() -> &'static Regex {
    regex!(
        r"^\s*-\s*(?:\[(?P<mark>[ xX])\]\s*)?\[\[(?P<id>[A-Z]{2,4}-\d{4})(?:\|(?P<alias>[^\]]+))?\]\]"
    )
}

fn date_stamp_regex() -> &'static Regex {
    regex!(r"✅\s*(\d{4}-\d{2}-\d{2})")
}

/// Contains one parsed index checkbox and its raw enclosing section label.
pub(super) struct ParsedIndexLine {
    pub id: TaskId,
    pub alias: Option<String>,
    pub state: IndexEntryState,
    /// Retains the raw section label without canonicalization.
    pub section: Option<TaskSection>,
    /// Uses a one-based line number within the index.
    pub line_number: NonZeroUsize,
}

/// Parses task checkbox lines with their raw enclosing H2 labels.
///
/// This representation mapping rejects duplicate task identities and applies no cap, eviction, or
/// normalization policy.
pub(super) fn parse_index_lines(
    index_path: &Path,
    text: &str,
) -> Result<Vec<ParsedIndexLine>, ObsidianStoreError> {
    let mut section = None;
    let mut lines = Vec::new();
    for (index, raw) in text.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(parsed) = parse_index_header(index_path, line, index + 1)? {
            section = Some(parsed);
            continue;
        }
        if let Some(parsed) = parse_index_task(index_path, line, index, section.as_ref())? {
            lines.push(parsed);
        }
    }
    reject_duplicate_task_ids(index_path, &lines)?;
    Ok(lines)
}

fn parse_index_header(
    index_path: &Path,
    line: &str,
    line_number: usize,
) -> Result<Option<TaskSection>, ObsidianStoreError> {
    let Some(header) = header_regex().captures(line) else {
        return Ok(None);
    };
    let value = header["label"].trim();
    TaskSection::try_new(value).map(Some).map_err(|source| {
        ObsidianStoreError::InvalidProjectIndexSection {
            path: index_path.to_path_buf(),
            line: line_number,
            value: value.to_string(),
            source,
        }
    })
}

fn parse_index_task(
    index_path: &Path,
    line: &str,
    index: usize,
    section: Option<&TaskSection>,
) -> Result<Option<ParsedIndexLine>, ObsidianStoreError> {
    let Some(task) = task_line_regex().captures(line) else {
        return Ok(None);
    };
    let Ok(id) = TaskId::try_new(&task["id"]) else {
        return Ok(None);
    };
    let alias = task.name("alias").map(|alias| alias.as_str().to_string());
    // A bare `- [[ID]]` link is open, like an unchecked checkbox.
    let is_done = matches!(task.name("mark").map(|mark| mark.as_str()), Some("x" | "X"));
    let state = if is_done {
        IndexEntryState::Done(parse_completion_date(index_path, line, index + 1)?)
    } else {
        IndexEntryState::Open
    };
    Ok(Some(ParsedIndexLine {
        id,
        alias,
        state,
        section: section.cloned(),
        line_number: NonZeroUsize::MIN.saturating_add(index),
    }))
}

fn parse_completion_date(
    index_path: &Path,
    line: &str,
    line_number: usize,
) -> Result<Option<AppDate>, ObsidianStoreError> {
    let Some(captures) = date_stamp_regex().captures(line) else {
        return Ok(None);
    };
    let value = captures[1].to_string();
    value.parse::<AppDate>().map(Some).map_err(|source| {
        ObsidianStoreError::InvalidProjectIndexDate {
            path: index_path.to_path_buf(),
            line: line_number,
            value,
            source,
        }
    })
}

fn reject_duplicate_task_ids(
    index_path: &Path,
    lines: &[ParsedIndexLine],
) -> Result<(), ObsidianStoreError> {
    let mut line_numbers_by_id = BTreeMap::<TaskId, Vec<usize>>::new();
    for line in lines {
        line_numbers_by_id
            .entry(line.id.clone())
            .or_default()
            .push(line.line_number.get());
    }
    if let Some((id, line_numbers)) = line_numbers_by_id
        .into_iter()
        .find(|(_, line_numbers)| line_numbers.len() > 1)
    {
        return Err(ObsidianStoreError::ProjectIndexTaskIdDuplicate {
            path: index_path.to_path_buf(),
            id,
            lines: line_numbers,
        });
    }
    Ok(())
}

/// Returns raw H2 labels in document order using the entry parser's header rules.
pub(super) fn parse_section_labels(
    index_path: &Path,
    text: &str,
) -> Result<Vec<TaskSection>, ObsidianStoreError> {
    text.split('\n')
        .enumerate()
        .filter_map(|(index, raw)| {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            header_regex()
                .captures(line)
                .map(|header| (index + 1, header["label"].trim().to_string()))
        })
        .map(|(line, value)| {
            TaskSection::try_new(&value).map_err(|source| {
                ObsidianStoreError::InvalidProjectIndexSection {
                    path: index_path.to_path_buf(),
                    line,
                    value,
                    source,
                }
            })
        })
        .collect()
}

fn render_entry_line(entry: &IndexEntry) -> String {
    match &entry.state {
        IndexEntryState::Open => format!("- [ ] [[{}]]", entry.id.as_ref()),
        IndexEntryState::Done(Some(date)) => {
            format!("- [x] [[{}]] ✅ {date}", entry.id.as_ref())
        }
        IndexEntryState::Done(None) => format!("- [x] [[{}]]", entry.id.as_ref()),
    }
}

/// Rewrites matching H2 labels while preserving all other bytes and the trailing newline.
fn rename_header_lines(content: &str, from: &str, to: &str) -> String {
    content
        .split('\n')
        .map(|line| {
            let stripped = line.strip_suffix('\r').unwrap_or(line);
            match header_regex().captures(stripped) {
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
    fn list_index_entries(&self, project: &Project) -> Result<Vec<IndexEntry>, ObsidianStoreError> {
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
        project: &Project,
        entry: &IndexEntry,
    ) -> Result<(), ObsidianStoreError> {
        let index_path = self.project_index_path(project)?;
        let index_dir = index_path.parent().unwrap_or(Path::new("."));
        if !index_dir.exists() {
            std::fs::create_dir_all(index_dir)
                .map_err(|source| ObsidianStoreError::CreateIndexDir { source })?;
        }
        let identity = Self::project_identity(project);
        let content = if index_path.is_file() {
            let content = read_index(&index_path)?;
            let file = MarkdownFile::from_source(index_path.clone(), content);
            let actual = parse_project_index_identity(&file)?;
            validate_project_index_identity(&index_path, &actual, &identity)?;
            file.into_source()
        } else {
            new_project_index_content(&identity)
        };
        let new_line = render_entry_line(entry);
        if let Some(existing) = parse_index_lines(&index_path, &content)?
            .into_iter()
            .find(|line| line.id == entry.id)
        {
            let updated = replace_line(&content, existing.line_number.get(), &new_line);
            return write_index(&index_path, &updated);
        }
        let (updated, created_section) = match entry.section.as_ref().and_then(KnownSection::parse)
        {
            Some(section) => {
                let created_section = (!section_exists(&content, section))
                    .then(|| entry.section.clone())
                    .flatten();
                (
                    add_section_block(&content, &format!("{new_line}\n"), section),
                    created_section,
                )
            }
            None => (add_link_to_index(&content, &new_line), None),
        };
        write_add_index_file(
            &index_path,
            &updated,
            project.title.as_ref(),
            created_section.as_ref(),
        )
    }

    /// Renames an H2 section label in place without applying application policy.
    fn rename_section_header(
        &self,
        project: &Project,
        from: &TaskSection,
        to: &TaskSection,
    ) -> Result<(), ObsidianStoreError> {
        let index_path = self.project_index_path(project)?;
        let content = read_index(&index_path)?;
        write_index(
            &index_path,
            &rename_header_lines(&content, from.as_ref(), to.as_ref()),
        )
    }

    fn delete_index_entry(&self, project: &Project, id: &TaskId) -> Result<(), ObsidianStoreError> {
        let Some((index_path, content)) = self.validated_project_index(project)? else {
            return Ok(());
        };
        let updated = remove_index_link(&content, id.as_ref());
        if updated == content {
            return Ok(());
        }
        write_index(&index_path, &updated)
    }
}

impl IndexEntryStore for ObsidianStore {
    type Error = ObsidianStoreError;

    fn list_index_entries(&self, project: &Project) -> Result<Vec<IndexEntry>, Self::Error> {
        ObsidianStore::list_index_entries(self, project)
    }

    fn upsert_index_entry(&self, project: &Project, entry: IndexEntry) -> Result<(), Self::Error> {
        ObsidianStore::upsert_index_entry(self, project, &entry)
    }

    fn delete_index_entry(&self, project: &Project, id: &TaskId) -> Result<(), Self::Error> {
        ObsidianStore::delete_index_entry(self, project, id)
    }
}

impl IndexSectionStore for ObsidianStore {
    type Error = ObsidianStoreError;

    fn list_index_sections(&self, project: &Project) -> Result<Vec<TaskSection>, Self::Error> {
        let Some((index_path, text)) = self.validated_project_index(project)? else {
            return Ok(Vec::new());
        };
        parse_section_labels(&index_path, &text)
    }

    fn rename_index_section(
        &self,
        project: &Project,
        current_label: &TaskSection,
        new_label: &TaskSection,
    ) -> Result<(), Self::Error> {
        self.rename_section_header(project, current_label, new_label)
    }
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, path::Path};

    use pwf_application::ports::task_record::{IndexEntry, IndexEntryState};
    use pwf_models::{AppDate, task::TaskId};

    use super::{ObsidianStoreError, parse_index_lines, render_entry_line, replace_line};

    #[test]
    fn done_entry_replaces_only_its_index_line() -> anyhow::Result<()> {
        let entry = IndexEntry {
            id: TaskId::try_new("FOO-0001")?,
            state: IndexEntryState::Done(Some("2026-07-29".parse::<AppDate>()?)),
            section: None,
        };
        let index = "# foo\n\n- [ ] [[FOO-0001]]\n- [ ] [[FOO-0002]]\n";

        assert_eq!(
            replace_line(index, 3, &render_entry_line(&entry)),
            "# foo\n\n- [x] [[FOO-0001]] ✅ 2026-07-29\n- [ ] [[FOO-0002]]\n"
        );
        Ok(())
    }

    #[test]
    fn index_parser_accepts_two_to_four_letter_project_ids() {
        let index = "- [ ] [[P-0001]]\n- [ ] [[PW-0002]]\n- [ ] [[FOO-0003]]\n- [ ] [[TOOL-0004]]\n- [ ] [[TOOLS-0005]]\n";

        let lines = parse_index_lines(Path::new("index.md"), index).unwrap();

        assert_eq!(
            lines.into_iter().map(|line| line.id).collect::<Vec<_>>(),
            [
                TaskId::try_new("PW-0002").unwrap(),
                TaskId::try_new("FOO-0003").unwrap(),
                TaskId::try_new("TOOL-0004").unwrap(),
            ]
        );
    }

    #[test]
    fn index_parser_rejects_an_invalid_completion_date() {
        let error = parse_index_lines(Path::new("index.md"), "- [x] [[FOO-0001]] ✅ 2026-02-30\n")
            .err()
            .unwrap();

        assert_matches!(
            error,
            ObsidianStoreError::InvalidProjectIndexDate {
                line: 1,
                ref value,
                ..
            } if value == "2026-02-30"
        );
    }
}
