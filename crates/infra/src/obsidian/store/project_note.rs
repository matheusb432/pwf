use std::{fmt::Write as _, path::Path};

use pwf_application::ports::{
    app_record::AppRecordStore,
    project_note::{NewProjectNote, ProjectNotePatch, ProjectNoteStore},
};
use pwf_models::{
    note::{NoteId, ProjectNote},
    pending_work::{ProjectId, ProjectName},
};
use regex::Regex;

use super::{ObsidianStore, ObsidianStoreError, fs::read_item_file};
use crate::obsidian::{
    frontmatter_text, fs_atomic,
    index_text::{add_note_link, remove_note_link},
    note_text,
};

impl AppRecordStore<ProjectNote> for ObsidianStore {
    type Error = ObsidianStoreError;

    fn get(&self, project: &ProjectName, id: &NoteId) -> Result<Option<ProjectNote>, Self::Error> {
        let path = self
            .project_paths
            .project_directory(project)?
            .join(note_file_name(id));
        if !path.exists() {
            return Ok(None);
        }
        let source = std::fs::read_to_string(path).map_err(|source| {
            ObsidianStoreError::ReadProjectNote {
                id: id.to_string(),
                source,
            }
        })?;
        Ok(Some(ProjectNote {
            id: id.clone(),
            topic: topic_of(&source),
        }))
    }

    fn list(&self, project: &ProjectName) -> Result<Vec<ProjectNote>, Self::Error> {
        let project_directory = self.project_paths.project_directory(project)?;
        let project_id = self.project_paths.project_identity(project)?.id();
        Ok(list_notes(project_directory, project_id))
    }

    fn insert(
        &self,
        project: &ProjectName,
        new: NewProjectNote,
    ) -> Result<ProjectNote, Self::Error> {
        let project_directory = self.project_paths.project_directory(project)?;
        let note_path = project_directory.join(note_file_name(&new.id));
        let source = note_content(project.as_ref(), &new);
        fs_atomic::write_text_atomic(&note_path, &source).map_err(|source| {
            ObsidianStoreError::WriteProjectNote {
                id: new.id.to_string(),
                source,
            }
        })?;

        let index_path = self.project_paths.project_index_path(project)?;
        let index = std::fs::read_to_string(&index_path).unwrap_or_default();
        write_index(&index_path, &add_note_link(&index, new.id.as_ref()))?;
        Ok(ProjectNote {
            id: new.id,
            topic: new.topic,
        })
    }

    fn update(
        &self,
        project: &ProjectName,
        id: &NoteId,
        patch: ProjectNotePatch,
    ) -> Result<(), Self::Error> {
        let note_path = self
            .project_paths
            .project_directory(project)?
            .join(note_file_name(id));
        if !note_path.exists() {
            return Err(note_not_found(project, id));
        }
        let source = std::fs::read_to_string(&note_path).map_err(|source| {
            ObsidianStoreError::ReadProjectNote {
                id: id.to_string(),
                source,
            }
        })?;
        fs_atomic::write_text_atomic(&note_path, &replace_topic(&source, &patch.topic)).map_err(
            |source| ObsidianStoreError::WriteProjectNote {
                id: id.to_string(),
                source,
            },
        )
    }

    fn delete(&self, project: &ProjectName, id: &NoteId) -> Result<(), Self::Error> {
        let note_path = self
            .project_paths
            .project_directory(project)?
            .join(note_file_name(id));
        if !note_path.exists() {
            return Err(note_not_found(project, id));
        }
        std::fs::remove_file(&note_path).map_err(|source| {
            ObsidianStoreError::RemoveProjectNote {
                id: id.to_string(),
                source,
            }
        })?;

        let index_path = self.project_paths.project_index_path(project)?;
        let index = std::fs::read_to_string(&index_path).unwrap_or_default();
        write_index(&index_path, &remove_note_link(&index, id.as_ref()))
    }
}

impl ProjectNoteStore for ObsidianStore {
    fn note_exists(
        &self,
        project: &ProjectName,
        id: &NoteId,
    ) -> Result<bool, <Self as AppRecordStore<ProjectNote>>::Error> {
        let note_path = self
            .project_paths
            .project_directory(project)?
            .join(note_file_name(id));
        note_path
            .try_exists()
            .map_err(|source| ObsidianStoreError::InspectProjectNote {
                id: id.to_string(),
                source,
            })
    }

    fn read_note_markdown(
        &self,
        locator: &str,
    ) -> Result<String, <Self as AppRecordStore<ProjectNote>>::Error> {
        read_item_file(Path::new(locator))
    }
}

fn list_notes(project_directory: &Path, project_id: &ProjectId) -> Vec<ProjectNote> {
    let pattern = format!(r"^{}-NOTE-(\d{{4}})$", regex::escape(project_id.as_ref()));
    let identifier_pattern = Regex::new(&pattern).unwrap();
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
        if !identifier_pattern.is_match(stem) {
            continue;
        }
        let Ok(id) = NoteId::try_new(stem) else {
            continue;
        };
        let source = std::fs::read_to_string(path).unwrap_or_default();
        notes.push(ProjectNote {
            id,
            topic: topic_of(&source),
        });
    }
    notes
}

fn note_file_name(id: &NoteId) -> String {
    format!("{id}.md")
}

fn topic_of(source: &str) -> String {
    let body = frontmatter_text::parse(source).body;
    body.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .or_else(|| body.lines().map(str::trim).find(|line| !line.is_empty()))
        .unwrap_or("")
        .to_string()
}

fn note_content(project: &str, note: &NewProjectNote) -> String {
    let mut source = String::from("---\ntype: note\n");
    let _ = writeln!(source, "project: {project}");
    let _ = writeln!(source, "created: {}", note.created.as_str());
    if let Some(domain) = &note.domain {
        let _ = writeln!(source, "domain: {}", yaml_string(domain));
    }
    if !note.tags.is_empty() {
        let _ = writeln!(source, "tags: {}", yaml_array(&note.tags));
    }
    if !note.sources.is_empty() {
        let _ = writeln!(source, "sources: {}", yaml_array(&note.sources));
    }
    if let Some(verified) = &note.verified {
        let _ = writeln!(source, "verified: {}", yaml_string(verified));
    }
    source.push_str("---\n\n");
    let _ = writeln!(source, "# {}\n", note.topic);
    let _ = writeln!(source, "> **TL;DR:** {}", note.tldr);
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
    serde_json::to_string(value).expect("serializing a string as JSON cannot fail")
}

fn yaml_array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| yaml_string(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn replace_topic(source: &str, topic: &str) -> String {
    let body_start = frontmatter_body_start(source);
    let mut line_start = body_start;
    for line in source[body_start..].split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        if content.starts_with("# ") {
            let line_end = line_start + content.len();
            return format!("{}# {topic}{}", &source[..line_start], &source[line_end..]);
        }
        line_start += line.len();
    }
    note_text::replace_body(source, topic)
}

fn frontmatter_body_start(source: &str) -> usize {
    let byte_order_mark = source
        .strip_prefix('\u{feff}')
        .map_or(0, |_| '\u{feff}'.len_utf8());
    let mut lines = source[byte_order_mark..].split_inclusive('\n');
    let Some(opening) = lines.next() else {
        return 0;
    };
    if opening.trim_end_matches(['\r', '\n']) != "---" {
        return 0;
    }

    let mut offset = byte_order_mark + opening.len();
    for line in lines {
        offset += line.len();
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return offset;
        }
    }
    0
}

fn note_not_found(project: &ProjectName, id: &NoteId) -> ObsidianStoreError {
    ObsidianStoreError::ProjectNoteNotFound {
        id: id.to_string(),
        project: project.to_string(),
    }
}

fn write_index(path: &Path, source: &str) -> Result<(), ObsidianStoreError> {
    fs_atomic::write_text_atomic(path, source).map_err(|source| {
        ObsidianStoreError::WriteProjectNoteIndex {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, fs, path::Path};

    use pwf_application::{
        note::remove_note::{self, RemoveNote},
        pending_work::ProjectRegistry,
        ports::{
            app_record::AppRecordStore,
            project_note::{NewProjectNote, ProjectNotePatch},
        },
    };
    use pwf_models::{
        note::{NoteId, ProjectNote},
        pending_work::{ProjectId, ProjectIndexIdentity, ProjectName, Timestamp},
    };

    use super::super::{ObsidianProject, ObsidianStore, ObsidianStoreError};

    fn store(tasks_path: &Path) -> ObsidianStore {
        ObsidianStore::new([ObsidianProject::new(
            ProjectIndexIdentity::new(
                ProjectId::try_new("PWF").unwrap(),
                ProjectName::try_new("pwf").unwrap(),
            ),
            tasks_path.to_path_buf(),
        )])
    }

    fn project() -> ProjectName {
        ProjectName::try_new("pwf").unwrap()
    }

    fn identifier(number: u32) -> NoteId {
        NoteId::try_new(format!("PWF-NOTE-{number:04}")).unwrap()
    }

    fn registry() -> ProjectRegistry {
        ProjectRegistry::new([(
            project(),
            Some("/repo/pwf".to_string()),
            Some("PWF".to_string()),
        )])
    }

    fn new_note(number: u32, topic: &str) -> NewProjectNote {
        NewProjectNote {
            id: identifier(number),
            topic: topic.to_string(),
            tldr: "A CLI flag needs a binary test only for an owned contract.".to_string(),
            why: Some("This protects real process-boundary failures.".to_string()),
            domain: Some("testing".to_string()),
            tags: vec!["cli".to_string(), "testing".to_string()],
            sources: vec!["PWF-0165 implementation evidence".to_string()],
            verified: Some("2026-07-30".to_string()),
            created: Timestamp::new("2026-07-26"),
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

        let inserted = <ObsidianStore as AppRecordStore<ProjectNote>>::insert(
            &store,
            &project(),
            new_note(1, "remember milk"),
        )
        .unwrap();

        assert_eq!(inserted.id.as_ref(), "PWF-NOTE-0001");
        assert_eq!(inserted.topic, "remember milk");
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
                "> **TL;DR:** A CLI flag needs a binary test only for an owned contract.\n\n",
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
            <ObsidianStore as AppRecordStore<ProjectNote>>::list(&store, &project()).unwrap(),
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

        <ObsidianStore as AppRecordStore<ProjectNote>>::update(
            &store,
            &project(),
            &identifier(1),
            ProjectNotePatch {
                topic: "new message".to_string(),
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
    fn update_changes_only_the_canonical_topic_with_windows_line_endings() {
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
            "# old topic\r\n\r\n",
            "> **TL;DR:** Preserve this.\r\n\r\n",
            "## Why it matters\r\n\r\n",
            "Keep every other byte.\r\n",
        );
        fs::write(tasks_path.join("PWF-NOTE-0001.md"), source).unwrap();
        let store = store(&tasks_path);

        <ObsidianStore as AppRecordStore<ProjectNote>>::update(
            &store,
            &project(),
            &identifier(1),
            ProjectNotePatch {
                topic: "new topic".to_string(),
            },
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(tasks_path.join("PWF-NOTE-0001.md")).unwrap(),
            source.replacen("# old topic", "# new topic", 1)
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

        let error = <ObsidianStore as AppRecordStore<ProjectNote>>::delete(
            &store,
            &project(),
            &identifier(1),
        )
        .unwrap_err();

        assert_matches!(
            error,
            ObsidianStoreError::ProjectNoteNotFound { ref id, ref project }
                if id == "PWF-NOTE-0001" && project == "pwf"
        );
        assert_eq!(fs::read_to_string(index_path).unwrap(), index);
    }

    #[test]
    fn remove_operation_deletes_non_utf8_note_and_preserves_other_index_content() {
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

        let removed = remove_note::execute(
            RemoveNote {
                project_identifier: "pwf".to_string(),
                id: "note-0001".to_string(),
            },
            &store,
            &registry(),
        )
        .unwrap();

        assert_eq!(removed.id.as_ref(), "PWF-NOTE-0001");
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

        let error = <ObsidianStore as AppRecordStore<ProjectNote>>::delete(
            &store,
            &project(),
            &identifier(1),
        )
        .unwrap_err();

        assert_matches!(error, ObsidianStoreError::WriteProjectNoteIndex { .. });
        assert!(!note_path.exists());
    }
}
