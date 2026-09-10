use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

use pwf_application::ports::project_note::{NewProjectNote, ProjectNotePatch, ProjectNotes};
use pwf_models::{
    note::{NoteId, NoteSource, NoteTag, NoteTitle, NoteTitleError, NoteVerification, ProjectNote},
    project::{Project, ProjectId, ProjectName},
};
use pwf_wire::{collection_edit::CollectionEdit, patch_field::PatchField, set_field::SetField};
use serde::Deserialize;
use serde_json::Value;

use super::{ObsidianStore, ObsidianStoreError};
use crate::{
    file_transaction::{FileSnapshot, FileTransaction, snapshot},
    obsidian::{MarkdownFile, markdown_line},
};

impl ProjectNotes for ObsidianStore {
    type Error = ObsidianStoreError;

    fn get_note(&self, project: &Project, id: &NoteId) -> Result<Option<ProjectNote>, Self::Error> {
        let path = self.project_note_path(project, id)?;
        if !path.exists() {
            return Ok(None);
        }
        let file =
            MarkdownFile::open(path).map_err(|source| ObsidianStoreError::ReadProjectNote {
                id: id.to_string(),
                source: source.into_io_error(),
            })?;
        project_note(id.clone(), &file).map(Some)
    }

    fn list_notes(&self, project: &Project) -> Result<Vec<ProjectNote>, Self::Error> {
        let project_directory = self.tasks_path(project)?;
        list_notes(
            &project_directory,
            &self.project_page_path(project)?,
            &project.id,
        )
    }

    fn insert_note(
        &self,
        project: &Project,
        new: NewProjectNote,
    ) -> Result<ProjectNote, Self::Error> {
        let note_path = self.project_note_path(project, &new.id)?;
        let source = note_content(project.title.as_ref(), &new);
        MarkdownFile::create_rendered_new(note_path, source).map_err(|source| {
            ObsidianStoreError::WriteProjectNote {
                id: new.id.to_string(),
                source: source.into_io_error(),
            }
        })?;

        Ok(ProjectNote {
            verified: new.verified,
            id: new.id,
            title: new.title,
        })
    }

    fn update_note(
        &self,
        project: &Project,
        id: &NoteId,
        patch: ProjectNotePatch,
    ) -> Result<(), Self::Error> {
        let note_path = self.project_note_path(project, id)?;
        if !note_path.exists() {
            return Err(note_not_found(&project.title, id));
        }
        let mut file = MarkdownFile::open(&note_path).map_err(|source| {
            ObsidianStoreError::ReadProjectNote {
                id: id.to_string(),
                source: source.into_io_error(),
            }
        })?;
        apply_note_patch(&mut file, id, patch)?;
        file.save()
            .map_err(|source| ObsidianStoreError::WriteProjectNote {
                id: id.to_string(),
                source: source.into_io_error(),
            })
    }

    fn delete_note(&self, project: &Project, id: &NoteId) -> Result<(), Self::Error> {
        let note_path = self.project_note_path(project, id)?;
        let note =
            snapshot(&note_path).map_err(|source| ObsidianStoreError::RemoveProjectNote {
                id: id.to_string(),
                source: std::io::Error::other(source),
            })?;
        let FileSnapshot::Present(note) = note else {
            return Err(note_not_found(&project.title, id));
        };
        let mut transaction = FileTransaction::new();
        transaction
            .remove(note)
            .and_then(|()| transaction.commit())
            .map_err(|source| ObsidianStoreError::RemoveProjectNote {
                id: id.to_string(),
                source: std::io::Error::other(source),
            })
    }
}

impl ObsidianStore {
    fn project_note_path(
        &self,
        project: &Project,
        id: &NoteId,
    ) -> Result<PathBuf, ObsidianStoreError> {
        let path = self.tasks_path(project)?.join(note_file_name(id));
        if path == self.project_page_path(project)? {
            return Err(ObsidianStoreError::ProjectPagePathReserved { path });
        }
        Ok(path)
    }
}

fn list_notes(
    project_directory: &Path,
    project_page_path: &Path,
    project_id: &ProjectId,
) -> Result<Vec<ProjectNote>, ObsidianStoreError> {
    let mut notes = Vec::new();
    let entries = match std::fs::read_dir(project_directory) {
        Ok(entries) => entries,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(notes),
        Err(source) => {
            return Err(ObsidianStoreError::ReadProjectNoteDirectory {
                path: project_directory.to_path_buf(),
                source,
            });
        }
    };
    for entry in entries {
        let entry = entry.map_err(|source| ObsidianStoreError::ReadProjectNoteDirectory {
            path: project_directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path == project_page_path
            || path.extension().and_then(|extension| extension.to_str()) != Some("md")
        {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Ok(id) = NoteId::try_new(stem) else {
            continue;
        };
        if id.project_id() != project_id {
            continue;
        }
        let file =
            MarkdownFile::open(path).map_err(|source| ObsidianStoreError::ReadProjectNote {
                id: id.to_string(),
                source: source.into_io_error(),
            })?;
        notes.push(project_note(id, &file)?);
    }
    Ok(notes)
}

fn note_file_name(id: &NoteId) -> String {
    format!("{id}.md")
}

fn title_of(file: &MarkdownFile) -> Result<NoteTitle, NoteTitleError> {
    let body = file.body();
    let title = body
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .or_else(|| body.lines().map(str::trim).find(|line| !line.is_empty()))
        .unwrap_or("");
    NoteTitle::try_new(title)
}

#[derive(Default, Deserialize)]
struct StoredVerification {
    #[serde(default)]
    verified: Value,
}

fn project_note(id: NoteId, file: &MarkdownFile) -> Result<ProjectNote, ObsidianStoreError> {
    let title = title_of(file).map_err(|source| ObsidianStoreError::InvalidProjectNoteTitle {
        id: id.to_string(),
        source,
    })?;
    let metadata = file
        .frontmatter::<StoredVerification>()
        .map_err(|source| ObsidianStoreError::FrontmatterParse {
            path: file.path().to_path_buf(),
            property: "verified",
            source,
        })?
        .unwrap_or_default();
    let verified = match metadata.verified {
        Value::Null => None,
        Value::String(value) => Some(value),
        Value::Array(value) if value.is_empty() => None,
        Value::Object(value) if value.is_empty() => None,
        value => Some(value.to_string()),
    }
    .and_then(|value| NoteVerification::try_new(value).ok());
    Ok(ProjectNote {
        id,
        title,
        verified,
    })
}

fn note_content(project: &str, note: &NewProjectNote) -> String {
    let mut source = String::from("---\ntype: note\n");
    let _ = writeln!(source, "project: {project}");
    let _ = writeln!(source, "created: {}", note.created);
    if let Some(domain) = &note.domain {
        let _ = writeln!(source, "domain: {}", yaml_string(domain.as_ref()));
    }
    if !note.tags.is_empty() {
        let _ = writeln!(source, "tags: {}", yaml_array(&note.tags));
    }
    if !note.sources.is_empty() {
        let _ = writeln!(source, "sources: {}", yaml_array(&note.sources));
    }
    if let Some(verified) = &note.verified {
        let _ = writeln!(source, "verified: {}", yaml_string(verified.as_ref()));
    }
    source.push_str("---\n\n");
    let _ = writeln!(source, "# {}\n", note.title);
    let _ = writeln!(source, "{}", note.content);
    if !note.sources.is_empty() {
        source.push_str("\n## Sources\n\n");
        for evidence in &note.sources {
            let _ = writeln!(source, "- {evidence}");
        }
    }
    source
}

fn yaml_string(value: &str) -> String {
    serde_json::Value::String(value.to_string()).to_string()
}

fn yaml_array(values: &[impl AsRef<str>]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| yaml_string(value.as_ref()))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn apply_note_patch(
    file: &mut MarkdownFile,
    id: &NoteId,
    patch: ProjectNotePatch,
) -> Result<(), ObsidianStoreError> {
    let ProjectNotePatch {
        title,
        content,
        domain,
        tags,
        sources,
        verified,
    } = patch;
    let resolved_tags = resolve_collection(tags, || stored_tags(file, id))?;
    let resolved_sources = resolve_collection(sources, || stored_sources(file, id))?;
    let body_start = file.source().len() - file.body().len();
    let mut source = file.source().to_string();
    if let SetField::Set(title) = title {
        source = replace_title(&source, body_start, &title);
    }
    if let SetField::Set(content) = content {
        source = replace_content(&source, body_start, content.as_ref());
    }
    if let Some(sources) = &resolved_sources {
        let rendered = sources
            .iter()
            .map(|source| format!("- {source}"))
            .collect::<Vec<_>>()
            .join("\n");
        source = edit_section(
            &source,
            body_start,
            "## Sources",
            (!sources.is_empty()).then_some(rendered.as_str()),
            None,
        );
    }
    file.replace_source(source);
    apply_patch_field(file, id, "domain", domain)?;
    apply_collection_update(file, id, "tags", resolved_tags)?;
    apply_collection_update(file, id, "sources", resolved_sources)?;
    apply_patch_field(file, id, "verified", verified)?;
    Ok(())
}

fn resolve_collection<T: Clone + PartialEq>(
    edit: CollectionEdit<Vec<T>>,
    current: impl FnOnce() -> Result<Vec<T>, ObsidianStoreError>,
) -> Result<Option<Vec<T>>, ObsidianStoreError> {
    match edit {
        CollectionEdit::Unchanged => Ok(None),
        CollectionEdit::Clear => Ok(Some(Vec::new())),
        CollectionEdit::Replace(values) => Ok(Some(unique(values))),
        CollectionEdit::Append(values) => {
            let mut merged = current()?;
            append_unique(&mut merged, values);
            Ok(Some(merged))
        }
    }
}

fn append_unique<T: PartialEq>(values: &mut Vec<T>, additions: Vec<T>) {
    for value in additions {
        if !values.contains(&value) {
            values.push(value);
        }
    }
}

fn unique<T: PartialEq>(values: Vec<T>) -> Vec<T> {
    let mut unique = Vec::new();
    for value in values {
        if !unique.contains(&value) {
            unique.push(value);
        }
    }
    unique
}

#[derive(Default, Deserialize)]
struct StoredTags {
    #[serde(default)]
    tags: Vec<String>,
}

fn stored_tags(file: &MarkdownFile, id: &NoteId) -> Result<Vec<NoteTag>, ObsidianStoreError> {
    let values = file
        .frontmatter::<StoredTags>()
        .map_err(|source| ObsidianStoreError::FrontmatterParse {
            path: file.path().to_path_buf(),
            property: "tags",
            source,
        })?
        .unwrap_or_default()
        .tags;
    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            NoteTag::try_new(value).map_err(|source| ObsidianStoreError::InvalidProjectNoteTag {
                id: id.to_string(),
                index,
                source,
            })
        })
        .collect()
}

#[derive(Default, Deserialize)]
struct StoredSources {
    #[serde(default)]
    sources: Vec<String>,
}

fn stored_sources(file: &MarkdownFile, id: &NoteId) -> Result<Vec<NoteSource>, ObsidianStoreError> {
    let values = file
        .frontmatter::<StoredSources>()
        .map_err(|source| ObsidianStoreError::FrontmatterParse {
            path: file.path().to_path_buf(),
            property: "sources",
            source,
        })?
        .unwrap_or_default()
        .sources;
    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            NoteSource::try_new(value).map_err(|source| {
                ObsidianStoreError::InvalidProjectNoteSource {
                    id: id.to_string(),
                    index,
                    source,
                }
            })
        })
        .collect()
}

fn apply_patch_field<T: ToString>(
    file: &mut MarkdownFile,
    id: &NoteId,
    property: &str,
    update: PatchField<T>,
) -> Result<(), ObsidianStoreError> {
    match update {
        PatchField::NoAction => Ok(()),
        PatchField::Set(value) => file
            .set_property(property, &value.to_string())
            .map_err(|source| project_note_edit_error(id, source)),
        PatchField::Clear => file
            .remove_property(property)
            .map(|_| ())
            .map_err(|source| project_note_edit_error(id, source)),
    }
}

fn apply_collection_update<T: ToString>(
    file: &mut MarkdownFile,
    id: &NoteId,
    property: &str,
    values: Option<Vec<T>>,
) -> Result<(), ObsidianStoreError> {
    let Some(values) = values else {
        return Ok(());
    };
    if values.is_empty() {
        return file
            .remove_property(property)
            .map(|_| ())
            .map_err(|source| project_note_edit_error(id, source));
    }
    let values = values
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    file.set_property(property, &values)
        .map_err(|source| project_note_edit_error(id, source))
}

fn project_note_edit_error(
    id: &NoteId,
    source: crate::obsidian::MarkdownFileError,
) -> ObsidianStoreError {
    ObsidianStoreError::EditProjectNote {
        id: id.to_string(),
        source,
    }
}

fn replace_title(source: &str, body_start: usize, title: &NoteTitle) -> String {
    let Some(line) = title_line(source, body_start) else {
        return source.to_string();
    };
    let replacement = if line.text.starts_with("# ") {
        format!("# {title}")
    } else {
        title.to_string()
    };
    format!(
        "{}{}{}",
        &source[..line.start],
        replacement,
        &source[line.start + line.text.len()..]
    )
}

fn replace_content(source: &str, body_start: usize, content: &str) -> String {
    let Some(title) = title_line(source, body_start) else {
        return source.to_string();
    };
    let section_start = markdown_line::lines(source)
        .filter(|line| line.start >= title.end)
        .find(|line| line.text.trim() == "## Sources")
        .map_or(source.len(), |line| line.start);
    let newline = body_newline(source, body_start);
    let content = normalize_newlines(content, newline);
    let suffix_separator = if section_start < source.len() {
        newline
    } else {
        ""
    };
    format!(
        "{}{newline}{content}{newline}{suffix_separator}{}",
        &source[..title.end],
        &source[section_start..]
    )
}

fn edit_section(
    source: &str,
    body_start: usize,
    heading: &str,
    value: Option<&str>,
    insert_before: Option<&str>,
) -> String {
    let newline = body_newline(source, body_start);
    let existing = markdown_line::lines(source)
        .filter(|line| line.start >= body_start)
        .find(|line| line.text.trim() == heading);
    if let Some(start) = existing {
        let end = markdown_line::lines(source)
            .filter(|line| line.start >= start.end)
            .find(|line| line.text.trim_start().starts_with("## "))
            .map_or(source.len(), |line| line.start);
        let replacement = value.map_or_else(String::new, |value| {
            render_section(heading, value, newline, end < source.len())
        });
        return format!(
            "{}{}{}",
            &source[..start.start],
            replacement,
            &source[end..]
        );
    }
    let Some(value) = value else {
        return source.to_string();
    };
    let position = insert_before
        .and_then(|target| {
            markdown_line::lines(source)
                .filter(|line| line.start >= body_start)
                .find(|line| line.text.trim() == target)
                .map(|line| line.start)
        })
        .unwrap_or(source.len());
    let mut prefix = source[..position].to_string();
    ensure_blank_line(&mut prefix, newline);
    let section = render_section(heading, value, newline, position < source.len());
    format!("{prefix}{section}{}", &source[position..])
}

fn render_section(heading: &str, value: &str, newline: &str, followed: bool) -> String {
    let value = normalize_newlines(value, newline);
    let trailing = if followed {
        format!("{newline}{newline}")
    } else {
        newline.to_string()
    };
    format!("{heading}{newline}{newline}{value}{trailing}")
}

fn ensure_blank_line(source: &mut String, newline: &str) {
    if !source.ends_with(newline) {
        source.push_str(newline);
    }
    let separator = format!("{newline}{newline}");
    if !source.ends_with(&separator) {
        source.push_str(newline);
    }
}

fn title_line(source: &str, body_start: usize) -> Option<markdown_line::MarkdownLine<'_>> {
    markdown_line::lines(source)
        .filter(|line| line.start >= body_start)
        .find(|line| line.text.starts_with("# "))
        .or_else(|| {
            markdown_line::lines(source)
                .filter(|line| line.start >= body_start)
                .find(|line| !line.text.trim().is_empty())
        })
}

fn body_newline(source: &str, body_start: usize) -> &'static str {
    markdown_line::lines(&source[body_start..])
        .find(|line| !line.newline.is_empty())
        .map_or("\n", |line| line.newline)
}

fn normalize_newlines(value: &str, newline: &str) -> String {
    let normalized = value.replace("\r\n", "\n").replace('\r', "\n");
    if newline == "\n" {
        normalized
    } else {
        normalized.replace('\n', newline)
    }
}

fn note_not_found(project: &ProjectName, id: &NoteId) -> ObsidianStoreError {
    ObsidianStoreError::ProjectNoteNotFound {
        id: id.to_string(),
        project: project.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, fs, path::Path};

    use pwf_application::ports::project_note::{NewProjectNote, ProjectNotePatch, ProjectNotes};
    use pwf_models::{
        note::{NoteContent, NoteDomain, NoteId, NoteSource, NoteTag, NoteTitle, NoteVerification},
        project::{
            HomeDirectory, Project, ProjectId, ProjectName, ProjectSource, ProjectSourceKind,
            ProjectSourceValue, ProjectTasks, ProjectTasksKind, ProjectTasksPath,
        },
    };
    use pwf_wire::{collection_edit::CollectionEdit, patch_field::PatchField, set_field::SetField};

    use super::super::{ObsidianStore, ObsidianStoreError};
    use crate::obsidian::MarkdownFile;

    #[derive(Debug, serde::Deserialize)]
    struct NoteFrontmatter {
        domain: Option<String>,
        #[serde(default)]
        tags: Vec<String>,
        #[serde(default)]
        sources: Vec<String>,
        verified: Option<String>,
    }

    fn store(tasks_path: &Path) -> ObsidianStore {
        ObsidianStore::new(HomeDirectory::new(tasks_path.to_path_buf()))
    }

    fn project(tasks_path: &Path) -> Project {
        Project {
            obsidian_vault: None,
            id: ProjectId::try_new("FOO").unwrap(),
            title: ProjectName::try_new("foo").unwrap(),
            source: Some(ProjectSource::new(
                ProjectSourceKind::Directory,
                ProjectSourceValue::try_new("/projects/foo").unwrap(),
            )),
            tasks: ProjectTasks::new(
                ProjectTasksKind::Directory,
                ProjectTasksPath::try_new(tasks_path.to_string_lossy()).unwrap(),
            ),
            created_at: "2026-07-25T00:00:00.000Z".parse().unwrap(),
            is_paused: false,
            snapshot_enabled: false,
        }
    }

    fn identifier(number: u32) -> NoteId {
        NoteId::try_new(format!("FOO-NOTE-{number:04}")).unwrap()
    }

    fn new_note(number: u32, title: &str) -> NewProjectNote {
        NewProjectNote {
            id: identifier(number),
            title: NoteTitle::try_new(title).unwrap(),
            content: NoteContent::try_new("A CLI flag needs a binary test only for an owned contract.\n\n- Preserve the process boundary.").unwrap(),
            domain: Some(NoteDomain::try_new("testing").unwrap()),
            tags: vec![
                NoteTag::try_new("cli").unwrap(),
                NoteTag::try_new("testing").unwrap(),
            ],
            sources: vec![
                NoteSource::try_new("FOO-0001 sample evidence").unwrap(),
            ],
            verified: Some(NoteVerification::try_new("2026-07-30").unwrap()),
            created: "2026-07-26".parse().unwrap(),
        }
    }

    #[test]
    fn insert_and_list_round_trip_the_note_representation_and_preserve_project_page() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir_all(&tasks_path).unwrap();
        let index_path = tasks_path.join("foo.md");
        fs::write(&index_path, "- [ ] [[FOO-0001|task]]\n").unwrap();
        let store = store(&tasks_path);

        let inserted =
            ProjectNotes::insert_note(&store, &project(&tasks_path), new_note(1, "remember milk"))
                .unwrap();

        assert_eq!(inserted.id.as_ref(), "FOO-NOTE-0001");
        assert_eq!(inserted.title.as_ref(), "remember milk");
        assert_eq!(
            fs::read_to_string(tasks_path.join("FOO-NOTE-0001.md")).unwrap(),
            concat!(
                "---\n",
                "type: note\n",
                "project: foo\n",
                "created: 2026-07-26\n",
                "domain: \"testing\"\n",
                "tags: [\"cli\", \"testing\"]\n",
                "sources: [\"FOO-0001 sample evidence\"]\n",
                "verified: \"2026-07-30\"\n",
                "---\n\n",
                "# remember milk\n\n",
                "A CLI flag needs a binary test only for an owned contract.\n\n",
                "- Preserve the process boundary.\n\n",
                "## Sources\n\n",
                "- FOO-0001 sample evidence\n",
            )
        );
        assert_eq!(
            fs::read_to_string(&index_path).unwrap(),
            "- [ ] [[FOO-0001|task]]\n"
        );
        assert_eq!(
            ProjectNotes::list_notes(&store, &project(&tasks_path)).unwrap(),
            vec![inserted]
        );
    }

    #[test]
    fn listing_uses_any_nonempty_verified_frontmatter_data() {
        let directory = tempfile::tempdir().unwrap();
        let store = store(directory.path());
        for (value, expected) in [
            ("", None),
            ("verified:\n", None),
            ("verified: null\n", None),
            ("verified: ''\n", None),
            ("verified: []\n", None),
            ("verified: {}\n", None),
            ("verified: 2026-09-08\n", Some("2026-09-08")),
            ("verified: false\n", Some("false")),
            ("verified: 0\n", Some("0")),
            ("verified:\n  by: matheus\n", Some(r#"{"by":"matheus"}"#)),
            ("verified:\n  - checked\n", Some(r#"["checked"]"#)),
        ] {
            fs::write(
                directory.path().join("FOO-NOTE-0001.md"),
                format!("---\ntype: note\n{value}---\n# sample\n"),
            )
            .unwrap();
            let notes = ProjectNotes::list_notes(&store, &project(directory.path())).unwrap();
            assert_eq!(
                notes[0].verified.as_ref().map(AsRef::as_ref),
                expected,
                "{value}"
            );
            assert_eq!(notes[0].is_verified(), expected.is_some(), "{value}");
        }
    }

    #[test]
    fn update_preserves_frontmatter_and_does_not_mutate_the_index() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir_all(&tasks_path).unwrap();
        let index_path = tasks_path.join("foo.md");
        fs::write(
            &index_path,
            "### Notes\n- [[FOO-NOTE-0001]]\n- [ ] [[FOO-0001|task]]\n",
        )
        .unwrap();
        fs::write(
            tasks_path.join("FOO-NOTE-0001.md"),
            "---\ntype: note\nproject: foo\ncreated: 2026-07-25\n---\n\nold message\n",
        )
        .unwrap();
        let index_before = fs::read(&index_path).unwrap();
        let store = store(&tasks_path);

        ProjectNotes::update_note(
            &store,
            &project(&tasks_path),
            &identifier(1),
            ProjectNotePatch {
                title: SetField::Set(NoteTitle::try_new("new message").unwrap()),
                ..ProjectNotePatch::default()
            },
        )
        .unwrap();

        assert_eq!(fs::read(&index_path).unwrap(), index_before);
        assert_eq!(
            fs::read_to_string(tasks_path.join("FOO-NOTE-0001.md")).unwrap(),
            "---\ntype: note\nproject: foo\ncreated: 2026-07-25\n---\n\nnew message\n"
        );
    }

    #[test]
    fn update_changes_only_the_canonical_title_with_windows_line_endings() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir_all(&tasks_path).unwrap();
        let index_path = tasks_path.join("foo.md");
        fs::write(&index_path, "### Notes\r\n\r\n- [[FOO-NOTE-0001]]\r\n").unwrap();
        let source = concat!(
            "---\r\n",
            "type: note\r\n",
            "project: foo\r\n",
            "created: 2026-07-25\r\n",
            "# frontmatter comment\r\n",
            "---\r\n\r\n",
            "# old title\r\n\r\n",
            "Preserve this.\r\n\r\n",
            "## Supporting detail\r\n\r\n",
            "Keep every other byte.\r\n",
        );
        fs::write(tasks_path.join("FOO-NOTE-0001.md"), source).unwrap();
        let store = store(&tasks_path);

        ProjectNotes::update_note(
            &store,
            &project(&tasks_path),
            &identifier(1),
            ProjectNotePatch {
                title: SetField::Set(NoteTitle::try_new("new title").unwrap()),
                ..ProjectNotePatch::default()
            },
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(tasks_path.join("FOO-NOTE-0001.md")).unwrap(),
            source.replacen("# old title", "# new title", 1)
        );
    }

    #[test]
    fn update_applies_explicit_scalar_and_collection_edits() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir_all(&tasks_path).unwrap();
        fs::write(tasks_path.join("foo.md"), "").unwrap();
        let store = store(&tasks_path);
        ProjectNotes::insert_note(&store, &project(&tasks_path), new_note(1, "old title")).unwrap();

        ProjectNotes::update_note(
            &store,
            &project(&tasks_path),
            &identifier(1),
            ProjectNotePatch {
                title: SetField::Set(NoteTitle::try_new("new title").unwrap()),
                content: SetField::Set(
                    NoteContent::try_new("New body.\n\n- Keep structure.").unwrap(),
                ),
                domain: PatchField::Clear,
                tags: CollectionEdit::Append(vec![
                    NoteTag::try_new("testing").unwrap(),
                    NoteTag::try_new("rust").unwrap(),
                ]),
                sources: CollectionEdit::Replace(vec![
                    NoteSource::try_new("PWF-0180 implementation").unwrap(),
                ]),
                verified: PatchField::Clear,
            },
        )
        .unwrap();

        let file = MarkdownFile::open(tasks_path.join("FOO-NOTE-0001.md")).unwrap();
        let metadata = file.frontmatter::<NoteFrontmatter>().unwrap().unwrap();
        assert_eq!(metadata.domain, None);
        assert_eq!(metadata.tags, ["cli", "testing", "rust"]);
        assert_eq!(metadata.sources, ["PWF-0180 implementation"]);
        assert_eq!(metadata.verified, None);
        assert_eq!(
            file.body(),
            concat!(
                "\n# new title\n\n",
                "New body.\n\n",
                "- Keep structure.\n\n",
                "## Sources\n\n",
                "- PWF-0180 implementation\n",
            )
        );
    }

    #[test]
    fn update_removes_only_fields_with_explicit_clear_operations() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir_all(&tasks_path).unwrap();
        fs::write(tasks_path.join("foo.md"), "").unwrap();
        let store = store(&tasks_path);
        ProjectNotes::insert_note(
            &store,
            &project(&tasks_path),
            new_note(1, "preserved title"),
        )
        .unwrap();

        ProjectNotes::update_note(
            &store,
            &project(&tasks_path),
            &identifier(1),
            ProjectNotePatch {
                domain: PatchField::Clear,
                tags: CollectionEdit::Clear,
                sources: CollectionEdit::Clear,
                verified: PatchField::Clear,
                ..ProjectNotePatch::default()
            },
        )
        .unwrap();

        let file = MarkdownFile::open(tasks_path.join("FOO-NOTE-0001.md")).unwrap();
        let metadata = file.frontmatter::<NoteFrontmatter>().unwrap().unwrap();
        assert_eq!(metadata.domain, None);
        assert!(metadata.tags.is_empty());
        assert!(metadata.sources.is_empty());
        assert_eq!(metadata.verified, None);
        assert_eq!(
            file.body(),
            concat!(
                "\n# preserved title\n\n",
                "A CLI flag needs a binary test only for an owned contract.\n\n",
                "- Preserve the process boundary.\n\n",
            )
        );
    }

    #[test]
    fn persisted_note_without_a_title_is_reported_as_invalid() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir_all(&tasks_path).unwrap();
        fs::write(
            tasks_path.join("FOO-NOTE-0001.md"),
            "---\ntype: note\nproject: foo\n---\n",
        )
        .unwrap();
        let store = store(&tasks_path);

        let error =
            ProjectNotes::get_note(&store, &project(&tasks_path), &identifier(1)).unwrap_err();

        assert_matches!(
            error,
            ObsidianStoreError::InvalidProjectNoteTitle { ref id, .. }
                if id == "FOO-NOTE-0001"
        );
    }

    #[test]
    fn missing_delete_is_reported_before_the_index_changes() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir_all(&tasks_path).unwrap();
        let index_path = tasks_path.join("foo.md");
        let index = "### Notes\n- [[FOO-NOTE-0001]]\n";
        fs::write(&index_path, index).unwrap();
        let store = store(&tasks_path);

        let error =
            ProjectNotes::delete_note(&store, &project(&tasks_path), &identifier(1)).unwrap_err();

        assert_matches!(
            error,
            ObsidianStoreError::ProjectNoteNotFound { ref id, ref project }
                if id == "FOO-NOTE-0001" && project == "foo"
        );
        assert_eq!(fs::read_to_string(index_path).unwrap(), index);
    }

    #[test]
    fn delete_removes_non_utf8_note_and_preserves_project_page() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir_all(&tasks_path).unwrap();
        let note_path = tasks_path.join("FOO-NOTE-0001.md");
        fs::write(&note_path, [0xff, 0xfe]).unwrap();
        let index_path = tasks_path.join("foo.md");
        fs::write(
            &index_path,
            "- [ ] [[FOO-0001]]\n\n### Notes\n\n- [[FOO-NOTE-0001]]\n",
        )
        .unwrap();
        let store = store(&tasks_path);

        ProjectNotes::delete_note(&store, &project(&tasks_path), &identifier(1)).unwrap();
        assert!(!note_path.exists());
        assert_eq!(
            fs::read_to_string(index_path).unwrap(),
            "- [ ] [[FOO-0001]]\n\n### Notes\n\n- [[FOO-NOTE-0001]]\n"
        );
    }

    #[test]
    fn delete_succeeds_when_the_project_page_cannot_be_read() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir_all(&tasks_path).unwrap();
        let note_path = tasks_path.join("FOO-NOTE-0001.md");
        fs::write(
            &note_path,
            "---\ntype: note\nproject: foo\ncreated: 2026-07-25\n---\n\nmessage\n",
        )
        .unwrap();
        fs::create_dir(tasks_path.join("foo.md")).unwrap();
        let store = store(&tasks_path);

        ProjectNotes::delete_note(&store, &project(&tasks_path), &identifier(1)).unwrap();

        assert!(!note_path.exists());
        assert!(tasks_path.join("foo.md").is_dir());
    }

    #[test]
    fn note_crud_preserves_authored_and_generated_pages() {
        for page in [
            &b"---\nid: FOO-9999\n---\n### Notes\n- [[FOO-NOTE-0001]]\n"[..],
            &b"---\ninvalid: [\n---\n"[..],
            &b"\xff\xfe"[..],
        ] {
            let directory = tempfile::tempdir().unwrap();
            let store = store(directory.path());
            let project = project(directory.path());
            for name in ["foo.md", "pwf-index.md"] {
                fs::write(directory.path().join(name), page).unwrap();
            }
            let note =
                ProjectNotes::insert_note(&store, &project, new_note(1, "Original")).unwrap();
            assert_eq!(
                ProjectNotes::get_note(&store, &project, &note.id).unwrap(),
                Some(note.clone())
            );
            assert_eq!(
                ProjectNotes::list_notes(&store, &project).unwrap(),
                std::slice::from_ref(&note)
            );
            ProjectNotes::update_note(
                &store,
                &project,
                &note.id,
                ProjectNotePatch {
                    title: SetField::Set(NoteTitle::try_new("Edited").unwrap()),
                    ..ProjectNotePatch::default()
                },
            )
            .unwrap();
            assert_eq!(
                ProjectNotes::get_note(&store, &project, &note.id)
                    .unwrap()
                    .unwrap()
                    .title
                    .as_ref(),
                "Edited"
            );
            ProjectNotes::delete_note(&store, &project, &note.id).unwrap();
            assert!(
                ProjectNotes::list_notes(&store, &project)
                    .unwrap()
                    .is_empty()
            );
            assert!(
                ProjectNotes::get_note(&store, &project, &note.id)
                    .unwrap()
                    .is_none()
            );
            for name in ["foo.md", "pwf-index.md"] {
                assert_eq!(fs::read(directory.path().join(name)).unwrap(), page);
            }
        }
    }

    #[test]
    fn note_crud_does_not_create_or_read_project_pages() {
        for unreadable_pages in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let store = store(directory.path());
            let project = project(directory.path());
            if unreadable_pages {
                fs::create_dir(directory.path().join("foo.md")).unwrap();
                fs::create_dir(directory.path().join("pwf-index.md")).unwrap();
            }
            let note =
                ProjectNotes::insert_note(&store, &project, new_note(1, "Original")).unwrap();
            ProjectNotes::update_note(
                &store,
                &project,
                &note.id,
                ProjectNotePatch {
                    title: SetField::Set(NoteTitle::try_new("Edited").unwrap()),
                    ..ProjectNotePatch::default()
                },
            )
            .unwrap();
            assert_eq!(
                ProjectNotes::get_note(&store, &project, &note.id)
                    .unwrap()
                    .unwrap()
                    .title
                    .as_ref(),
                "Edited"
            );
            ProjectNotes::delete_note(&store, &project, &note.id).unwrap();
            for name in ["foo.md", "pwf-index.md"] {
                let path = directory.path().join(name);
                assert_eq!(path.exists(), unreadable_pages);
                assert_eq!(path.is_dir(), unreadable_pages);
            }
        }
    }

    #[test]
    fn list_notes_reports_an_unreadable_directory_and_preserves_existing_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir(&tasks_path).unwrap();
        let snapshot = "- [ ] [[FOO-0001]]\n\n### Notes\n\n- [[FOO-NOTE-0001]]\n";
        fs::write(tasks_path.join("pwf-index.md"), snapshot).unwrap();
        let store = store(&tasks_path);
        let project = project(&tasks_path);
        let retained = directory.path().join("retained");
        fs::rename(&tasks_path, &retained).unwrap();
        fs::write(&tasks_path, "not a directory").unwrap();

        let error = ProjectNotes::list_notes(&store, &project).unwrap_err();

        assert_matches!(error, ObsidianStoreError::ReadProjectNoteDirectory { path, .. } if path == tasks_path);
        assert_eq!(
            fs::read_to_string(retained.join("pwf-index.md")).unwrap(),
            snapshot
        );
        assert_eq!(fs::read_to_string(&tasks_path).unwrap(), "not a directory");
    }

    #[test]
    fn list_notes_returns_empty_for_a_missing_directory() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("missing");
        let store = store(&tasks_path);
        assert!(
            ProjectNotes::list_notes(&store, &project(&tasks_path))
                .unwrap()
                .is_empty()
        );
        assert!(!tasks_path.exists());
    }
}
