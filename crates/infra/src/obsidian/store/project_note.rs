use std::{fmt::Write as _, path::Path};

use pwf_application::{
    AppRecordStore, NewProjectNote, ProjectNote, ProjectNotePatch, ProjectNoteStore,
};
use pwf_domain::{
    note::NoteId,
    pending_work::{ProjectName, ProjectPrefix},
};
use regex::Regex;

use super::{ObsidianStore, ObsidianStoreError};
use crate::obsidian::{
    frontmatter_text, fs_atomic,
    index_text::{add_note_link, remove_note_link},
};

impl AppRecordStore<ProjectNote> for ObsidianStore {
    type Error = ObsidianStoreError;

    fn get(&self, project: &ProjectName, id: &NoteId) -> Result<Option<ProjectNote>, Self::Error> {
        let path = self
            .project_paths
            .project_directory(project)?
            .join(id.file_name());
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
            message: message_of(&source),
        }))
    }

    fn list(&self, project: &ProjectName) -> Result<Vec<ProjectNote>, Self::Error> {
        let project_directory = self.project_paths.project_directory(project)?;
        let prefix = self.project_paths.project_identity(project)?.id();
        Ok(list_notes(project_directory, prefix))
    }

    fn insert(
        &self,
        project: &ProjectName,
        new: NewProjectNote,
    ) -> Result<ProjectNote, Self::Error> {
        let project_directory = self.project_paths.project_directory(project)?;
        let note_path = project_directory.join(new.id.file_name());
        let message = new.message.trim();
        let source = note_content(project.as_ref(), new.created.as_str(), message);
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
            message: message.to_string(),
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
            .join(id.file_name());
        if !note_path.exists() {
            return Err(note_not_found(project, id));
        }
        let source = std::fs::read_to_string(&note_path).map_err(|source| {
            ObsidianStoreError::ReadProjectNote {
                id: id.to_string(),
                source,
            }
        })?;
        fs_atomic::write_text_atomic(&note_path, &replace_body(&source, &patch.message)).map_err(
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
            .join(id.file_name());
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
            .join(id.file_name());
        note_path
            .try_exists()
            .map_err(|source| ObsidianStoreError::InspectProjectNote {
                id: id.to_string(),
                source,
            })
    }
}

fn list_notes(project_directory: &Path, prefix: &ProjectPrefix) -> Vec<ProjectNote> {
    let pattern = format!(r"^{}-NOTE-(\d{{4}})$", regex::escape(prefix.as_ref()));
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
            message: message_of(&source),
        });
    }
    notes
}

fn message_of(source: &str) -> String {
    frontmatter_text::parse(source)
        .body
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .to_string()
}

fn note_content(project: &str, created: &str, message: &str) -> String {
    let mut source = String::from("---\ntype: note\n");
    let _ = writeln!(source, "project: {project}");
    let _ = writeln!(source, "created: {created}");
    source.push_str("---\n\n");
    source.push_str(message);
    source.push('\n');
    source
}

fn replace_body(source: &str, message: &str) -> String {
    let Some((frontmatter, _)) = source.split_once("\n\n") else {
        return format!("{}\n", message.trim());
    };
    format!("{frontmatter}\n\n{}\n", message.trim())
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
        AppRecordStore, NewProjectNote, ProjectNote, ProjectNotePatch,
        note::remove::{self, RemoveNote},
        pending_work::ProjectRegistry,
    };
    use pwf_domain::{
        note::NoteId,
        pending_work::{ProjectIndexIdentity, ProjectName, ProjectPrefix, Timestamp},
    };

    use super::super::{ObsidianProject, ObsidianStore, ObsidianStoreError};

    fn store(tasks_path: &Path) -> ObsidianStore {
        ObsidianStore::new([ObsidianProject::new(
            ProjectIndexIdentity::new(
                ProjectPrefix::try_new("PWF").unwrap(),
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

    fn new_note(number: u32, message: &str) -> NewProjectNote {
        NewProjectNote {
            id: identifier(number),
            message: message.to_string(),
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
            new_note(1, " remember milk "),
        )
        .unwrap();

        assert_eq!(inserted.id.as_ref(), "PWF-NOTE-0001");
        assert_eq!(inserted.message, "remember milk");
        assert_eq!(
            fs::read_to_string(tasks_path.join("PWF-NOTE-0001.md")).unwrap(),
            "---\ntype: note\nproject: pwf\ncreated: 2026-07-26\n---\n\nremember milk\n"
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
                message: " new message ".to_string(),
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

        let removed = remove::execute(
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
