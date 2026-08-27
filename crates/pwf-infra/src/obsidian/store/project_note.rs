use std::{fmt::Write as _, path::Path};

use pwf_application::ports::project_note::{NewProjectNote, ProjectNotePatch, ProjectNoteStore};
use pwf_models::{
    note::{NoteId, NoteTitle, NoteTitleError, ProjectNote},
    project::{Project, ProjectId, ProjectName},
};

use super::{ObsidianStore, ObsidianStoreError, fs::read_task_file};
use crate::obsidian::{
    MarkdownFile,
    index_text::{add_note_link, remove_note_link},
    markdown_line, note_text,
};

impl ProjectNoteStore for ObsidianStore {
    type Error = ObsidianStoreError;

    fn get_note(&self, project: &Project, id: &NoteId) -> Result<Option<ProjectNote>, Self::Error> {
        let path = self.tasks_path(project)?.join(note_file_name(id));
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
        list_notes(&project_directory, &project.id)
    }

    fn insert_note(
        &self,
        project: &Project,
        new: NewProjectNote,
    ) -> Result<ProjectNote, Self::Error> {
        let project_directory = self.tasks_path(project)?;
        let note_path = project_directory.join(note_file_name(&new.id));
        let source = note_content(project.title.as_ref(), &new);
        MarkdownFile::create_rendered_new(note_path, source).map_err(|source| {
            ObsidianStoreError::WriteProjectNote {
                id: new.id.to_string(),
                source: source.into_io_error(),
            }
        })?;

        let index_path = self.project_index_path(project)?;
        let index = MarkdownFile::open(&index_path)
            .map(MarkdownFile::into_source)
            .unwrap_or_default();
        write_index(&index_path, &add_note_link(&index, new.id.as_ref()))?;
        Ok(ProjectNote {
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
        let note_path = self.tasks_path(project)?.join(note_file_name(id));
        if !note_path.exists() {
            return Err(note_not_found(&project.title, id));
        }
        let mut file = MarkdownFile::open(&note_path).map_err(|source| {
            ObsidianStoreError::ReadProjectNote {
                id: id.to_string(),
                source: source.into_io_error(),
            }
        })?;
        let updated = replace_title(&file, &patch.title);
        file.replace_source(updated);
        file.save()
            .map_err(|source| ObsidianStoreError::WriteProjectNote {
                id: id.to_string(),
                source: source.into_io_error(),
            })
    }

    fn delete_note(&self, project: &Project, id: &NoteId) -> Result<(), Self::Error> {
        let note_path = self.tasks_path(project)?.join(note_file_name(id));
        if !note_path.exists() {
            return Err(note_not_found(&project.title, id));
        }
        std::fs::remove_file(&note_path).map_err(|source| {
            ObsidianStoreError::RemoveProjectNote {
                id: id.to_string(),
                source,
            }
        })?;

        let index_path = self.project_index_path(project)?;
        let index = MarkdownFile::open(&index_path)
            .map(MarkdownFile::into_source)
            .unwrap_or_default();
        write_index(&index_path, &remove_note_link(&index, id.as_ref()))
    }
    fn note_exists(&self, project: &Project, id: &NoteId) -> Result<bool, Self::Error> {
        let note_path = self.tasks_path(project)?.join(note_file_name(id));
        note_path
            .try_exists()
            .map_err(|source| ObsidianStoreError::InspectProjectNote {
                id: id.to_string(),
                source,
            })
    }

    fn read_note_markdown(
        &self,
        locator: &pwf_wire::task::TaskNotePath,
    ) -> Result<String, Self::Error> {
        read_task_file(locator.as_path())
    }
}

fn list_notes(
    project_directory: &Path,
    project_id: &ProjectId,
) -> Result<Vec<ProjectNote>, ObsidianStoreError> {
    let mut notes = Vec::new();
    for entry in std::fs::read_dir(project_directory)
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
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

fn project_note(id: NoteId, file: &MarkdownFile) -> Result<ProjectNote, ObsidianStoreError> {
    let title = title_of(file).map_err(|source| ObsidianStoreError::InvalidProjectNoteTitle {
        id: id.to_string(),
        source,
    })?;
    Ok(ProjectNote { id, title })
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
    if let Some(why) = &note.why {
        let _ = write!(source, "\n## Why it matters\n\n{why}\n");
    }
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

fn replace_title(file: &MarkdownFile, title: &NoteTitle) -> String {
    let source = file.source();
    let body_start = source.len() - file.body().len();
    for line in markdown_line::lines(source).filter(|line| line.start >= body_start) {
        if line.text.starts_with("# ") {
            let line_end = line.start + line.text.len();
            return format!("{}# {title}{}", &source[..line.start], &source[line_end..]);
        }
    }
    note_text::replace_body(source, title.as_ref())
}

fn note_not_found(project: &ProjectName, id: &NoteId) -> ObsidianStoreError {
    ObsidianStoreError::ProjectNoteNotFound {
        id: id.to_string(),
        project: project.to_string(),
    }
}

fn write_index(path: &Path, source: &str) -> Result<(), ObsidianStoreError> {
    MarkdownFile::write_rendered(path.to_path_buf(), source.to_string()).map_err(|source| {
        ObsidianStoreError::WriteProjectNoteIndex {
            path: path.to_path_buf(),
            source: source.into_io_error(),
        }
    })
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, fs, path::Path};

    use pwf_application::ports::project_note::{
        NewProjectNote, ProjectNotePatch, ProjectNoteStore,
    };
    use pwf_models::{
        note::{
            NoteContent, NoteDomain, NoteId, NoteSource, NoteTag, NoteTitle, NoteVerification,
            NoteWhy,
        },
        project::{
            HomeDirectory, Project, ProjectId, ProjectName, ProjectSource, ProjectSourceKind,
            ProjectSourceValue, ProjectTasks, ProjectTasksKind, ProjectTasksPath,
        },
    };

    use super::super::{ObsidianStore, ObsidianStoreError};

    fn store(tasks_path: &Path) -> ObsidianStore {
        ObsidianStore::new(HomeDirectory::new(tasks_path.to_path_buf()))
    }

    fn project(tasks_path: &Path) -> Project {
        Project {
            id: ProjectId::try_new("PWF").unwrap(),
            title: ProjectName::try_new("pwf").unwrap(),
            source: ProjectSource::new(
                ProjectSourceKind::Directory,
                ProjectSourceValue::try_new("/projects/pwf").unwrap(),
            ),
            tasks: ProjectTasks::new(
                ProjectTasksKind::Directory,
                ProjectTasksPath::try_new(tasks_path.to_string_lossy()).unwrap(),
            ),
            created_at: "2026-07-25T00:00:00.000Z".parse().unwrap(),
            is_paused: false,
        }
    }

    fn identifier(number: u32) -> NoteId {
        NoteId::try_new(format!("PWF-NOTE-{number:04}")).unwrap()
    }

    fn new_note(number: u32, title: &str) -> NewProjectNote {
        NewProjectNote {
            id: identifier(number),
            title: NoteTitle::try_new(title).unwrap(),
            content: NoteContent::try_new("A CLI flag needs a binary test only for an owned contract.\n\n- Preserve the process boundary.").unwrap(),
            why: Some(
                NoteWhy::try_new("This protects real process-boundary failures.").unwrap(),
            ),
            domain: Some(NoteDomain::try_new("testing").unwrap()),
            tags: vec![
                NoteTag::try_new("cli").unwrap(),
                NoteTag::try_new("testing").unwrap(),
            ],
            sources: vec![
                NoteSource::try_new("PWF-0165 implementation evidence").unwrap(),
            ],
            verified: Some(NoteVerification::try_new("2026-07-30").unwrap()),
            created: "2026-07-26".parse().unwrap(),
        }
    }

    #[test]
    fn insert_and_list_round_trip_the_note_representation_and_index_link() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir_all(&tasks_path).unwrap();
        let index_path = tasks_path.join("pwf.md");
        fs::write(&index_path, "- [ ] [[PWF-0001|task]]\n").unwrap();
        let store = store(&tasks_path);

        let inserted = ProjectNoteStore::insert_note(
            &store,
            &project(&tasks_path),
            new_note(1, "remember milk"),
        )
        .unwrap();

        assert_eq!(inserted.id.as_ref(), "PWF-NOTE-0001");
        assert_eq!(inserted.title.as_ref(), "remember milk");
        assert_eq!(
            fs::read_to_string(tasks_path.join("PWF-NOTE-0001.md")).unwrap(),
            concat!(
                "---\n",
                "type: note\n",
                "project: pwf\n",
                "created: 2026-07-26\n",
                "domain: \"testing\"\n",
                "tags: [\"cli\", \"testing\"]\n",
                "sources: [\"PWF-0165 implementation evidence\"]\n",
                "verified: \"2026-07-30\"\n",
                "---\n\n",
                "# remember milk\n\n",
                "A CLI flag needs a binary test only for an owned contract.\n\n",
                "- Preserve the process boundary.\n\n",
                "## Why it matters\n\n",
                "This protects real process-boundary failures.\n\n",
                "## Sources\n\n",
                "- PWF-0165 implementation evidence\n",
            )
        );
        assert_eq!(
            fs::read_to_string(&index_path).unwrap(),
            "- [ ] [[PWF-0001|task]]\n\n### Notes\n\n- [[PWF-NOTE-0001]]\n"
        );
        assert_eq!(
            ProjectNoteStore::list_notes(&store, &project(&tasks_path)).unwrap(),
            vec![inserted]
        );
    }

    #[test]
    fn update_preserves_frontmatter_and_does_not_mutate_the_index() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir_all(&tasks_path).unwrap();
        let index_path = tasks_path.join("pwf.md");
        fs::write(
            &index_path,
            "### Notes\n- [[PWF-NOTE-0001]]\n- [ ] [[PWF-0001|task]]\n",
        )
        .unwrap();
        fs::write(
            tasks_path.join("PWF-NOTE-0001.md"),
            "---\ntype: note\nproject: pwf\ncreated: 2026-07-25\n---\n\nold message\n",
        )
        .unwrap();
        let index_before = fs::read(&index_path).unwrap();
        let store = store(&tasks_path);

        ProjectNoteStore::update_note(
            &store,
            &project(&tasks_path),
            &identifier(1),
            ProjectNotePatch {
                title: NoteTitle::try_new("new message").unwrap(),
            },
        )
        .unwrap();

        assert_eq!(fs::read(&index_path).unwrap(), index_before);
        assert_eq!(
            fs::read_to_string(tasks_path.join("PWF-NOTE-0001.md")).unwrap(),
            "---\ntype: note\nproject: pwf\ncreated: 2026-07-25\n---\n\nnew message\n"
        );
    }

    #[test]
    fn update_changes_only_the_canonical_title_with_windows_line_endings() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir_all(&tasks_path).unwrap();
        let index_path = tasks_path.join("pwf.md");
        fs::write(&index_path, "### Notes\r\n\r\n- [[PWF-NOTE-0001]]\r\n").unwrap();
        let source = concat!(
            "---\r\n",
            "type: note\r\n",
            "project: pwf\r\n",
            "created: 2026-07-25\r\n",
            "# frontmatter comment\r\n",
            "---\r\n\r\n",
            "# old title\r\n\r\n",
            "Preserve this.\r\n\r\n",
            "## Why it matters\r\n\r\n",
            "Keep every other byte.\r\n",
        );
        fs::write(tasks_path.join("PWF-NOTE-0001.md"), source).unwrap();
        let store = store(&tasks_path);

        ProjectNoteStore::update_note(
            &store,
            &project(&tasks_path),
            &identifier(1),
            ProjectNotePatch {
                title: NoteTitle::try_new("new title").unwrap(),
            },
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(tasks_path.join("PWF-NOTE-0001.md")).unwrap(),
            source.replacen("# old title", "# new title", 1)
        );
    }

    #[test]
    fn persisted_note_without_a_title_is_reported_as_invalid() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir_all(&tasks_path).unwrap();
        fs::write(
            tasks_path.join("PWF-NOTE-0001.md"),
            "---\ntype: note\nproject: pwf\n---\n",
        )
        .unwrap();
        let store = store(&tasks_path);

        let error =
            ProjectNoteStore::get_note(&store, &project(&tasks_path), &identifier(1)).unwrap_err();

        assert_matches!(
            error,
            ObsidianStoreError::InvalidProjectNoteTitle { ref id, .. }
                if id == "PWF-NOTE-0001"
        );
    }

    #[test]
    fn missing_delete_is_reported_before_the_index_changes() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir_all(&tasks_path).unwrap();
        let index_path = tasks_path.join("pwf.md");
        let index = "### Notes\n- [[PWF-NOTE-0001]]\n";
        fs::write(&index_path, index).unwrap();
        let store = store(&tasks_path);

        let error = ProjectNoteStore::delete_note(&store, &project(&tasks_path), &identifier(1))
            .unwrap_err();

        assert_matches!(
            error,
            ObsidianStoreError::ProjectNoteNotFound { ref id, ref project }
                if id == "PWF-NOTE-0001" && project == "pwf"
        );
        assert_eq!(fs::read_to_string(index_path).unwrap(), index);
    }

    #[test]
    fn delete_removes_non_utf8_note_and_preserves_other_index_content() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir_all(&tasks_path).unwrap();
        let note_path = tasks_path.join("PWF-NOTE-0001.md");
        fs::write(&note_path, [0xff, 0xfe]).unwrap();
        let index_path = tasks_path.join("pwf.md");
        fs::write(
            &index_path,
            "- [ ] [[PWF-0001]]\n\n### Notes\n\n- [[PWF-NOTE-0001]]\n",
        )
        .unwrap();
        let store = store(&tasks_path);

        ProjectNoteStore::delete_note(&store, &project(&tasks_path), &identifier(1)).unwrap();
        assert!(!note_path.exists());
        assert_eq!(
            fs::read_to_string(index_path).unwrap(),
            "- [ ] [[PWF-0001]]\n\n### Notes\n"
        );
    }

    #[test]
    fn delete_removes_the_note_before_an_index_write_failure() {
        let directory = tempfile::tempdir().unwrap();
        let tasks_path = directory.path().join("tasks");
        fs::create_dir_all(&tasks_path).unwrap();
        let note_path = tasks_path.join("PWF-NOTE-0001.md");
        fs::write(
            &note_path,
            "---\ntype: note\nproject: pwf\ncreated: 2026-07-25\n---\n\nmessage\n",
        )
        .unwrap();
        fs::create_dir(tasks_path.join("pwf.md")).unwrap();
        let store = store(&tasks_path);

        let error = ProjectNoteStore::delete_note(&store, &project(&tasks_path), &identifier(1))
            .unwrap_err();

        assert_matches!(error, ObsidianStoreError::WriteProjectNoteIndex { .. });
        assert!(!note_path.exists());
    }
}
