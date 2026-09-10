use std::fmt::Write as _;

use pwf_application::ports::{
    project_snapshot::ProjectSnapshotWriter, task_vault::TaskSummaryRecord,
};
use pwf_models::{note::ProjectNote, project::Project, task::TaskStatus};

use super::{ObsidianStore, ObsidianStoreError};
use crate::{
    file_transaction::{FileTransaction, snapshot},
    obsidian::PROJECT_SNAPSHOT_FILE_NAME,
};

impl ProjectSnapshotWriter for ObsidianStore {
    type Error = ObsidianStoreError;

    fn write_project_snapshot(
        &self,
        project: &Project,
        tasks: &[TaskSummaryRecord],
        notes: &[ProjectNote],
    ) -> Result<(), Self::Error> {
        let directory = self.tasks_path(project)?;
        let path = directory.join(PROJECT_SNAPSHOT_FILE_NAME);
        if path == self.project_page_path(project)? {
            return Err(ObsidianStoreError::ProjectPagePathReserved { path });
        }
        std::fs::create_dir_all(&directory)
            .map_err(|source| ObsidianStoreError::CreateProjectDir { source })?;
        let mut transaction = FileTransaction::new();
        snapshot(&path)
            .and_then(|current| {
                transaction.replace(
                    current,
                    render_snapshot(tasks, notes)
                        .into_bytes()
                        .into_boxed_slice(),
                )
            })
            .and_then(|()| transaction.commit())
            .map_err(|source| ObsidianStoreError::WriteProjectSnapshot {
                path,
                source: std::io::Error::other(source),
            })
    }
}

fn render_snapshot(tasks: &[TaskSummaryRecord], notes: &[ProjectNote]) -> String {
    let mut tasks: Vec<_> = tasks.iter().collect();
    tasks.sort_unstable_by(|left, right| right.id.cmp(&left.id));
    let mut notes: Vec<_> = notes.iter().collect();
    notes.sort_unstable_by(|left, right| right.id.cmp(&left.id));
    let mut source = String::new();
    for task in tasks {
        let checkbox = match task.status {
            TaskStatus::Active => ' ',
            TaskStatus::Done | TaskStatus::Cancelled => 'x',
        };
        let _ = writeln!(source, "- [{checkbox}] [[{}]]", task.id);
    }
    if !source.is_empty() {
        source.push('\n');
    }
    source.push_str("### Notes\n\n");
    for note in notes {
        let _ = writeln!(source, "- [[{}]]", note.id);
    }
    source
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, fs, path::Path};

    use pwf_application::project::refresh_project_snapshot;
    use pwf_models::{
        note::{NoteId, NoteTitle},
        project::{
            HomeDirectory, ProjectId, ProjectName, ProjectTasks, ProjectTasksKind, ProjectTasksPath,
        },
    };

    use super::*;

    fn project(directory: &Path) -> Project {
        Project {
            id: ProjectId::try_new("FOO").unwrap(),
            title: ProjectName::try_new("foo").unwrap(),
            source: None,
            tasks: ProjectTasks::new(
                ProjectTasksKind::Directory,
                ProjectTasksPath::try_new(directory.to_string_lossy()).unwrap(),
            ),
            obsidian_vault: None,
            created_at: "2026-09-10T00:00:00Z".parse().unwrap(),
            is_paused: false,
            snapshot_enabled: true,
        }
    }

    fn summary(id: &str, status: TaskStatus) -> TaskSummaryRecord {
        TaskSummaryRecord {
            id: id.parse().unwrap(),
            title: "Task".to_string(),
            status,
            created_at: None,
            tags: None,
            effort: None,
            priority: None,
        }
    }

    fn note(id: &str) -> ProjectNote {
        ProjectNote {
            id: NoteId::try_new(id).unwrap(),
            title: NoteTitle::try_new("Note").unwrap(),
            verified: None,
        }
    }

    #[test]
    fn snapshot_writes_sorted_checkboxes_and_note_links_without_changing_authored_files() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObsidianStore::new(HomeDirectory::new(directory.path().to_path_buf()));
        let project = project(directory.path());
        let page = b"---\ninvalid: [\n---\nAuthored content\n\xff";
        fs::write(directory.path().join("foo.md"), page).unwrap();
        let task = "---\nid: FOO-0001\nstatus: active\n---\nTask body\n";
        fs::write(directory.path().join("FOO-0001.md"), task).unwrap();
        let note_body = "# Authored note\n";
        fs::write(directory.path().join("FOO-NOTE-0001.md"), note_body).unwrap();
        let tasks = [
            summary("FOO-0002", TaskStatus::Done),
            summary("FOO-0001", TaskStatus::Active),
            summary("FOO-0003", TaskStatus::Cancelled),
        ];
        let notes = [
            note("FOO-NOTE-0001"),
            note("FOO-NOTE-0003"),
            note("FOO-NOTE-0002"),
        ];
        store
            .write_project_snapshot(&project, &tasks, &notes)
            .unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("pwf-index.md")).unwrap(),
            "- [x] [[FOO-0003]]\n- [x] [[FOO-0002]]\n- [ ] [[FOO-0001]]\n\n### Notes\n\n- [[FOO-NOTE-0003]]\n- [[FOO-NOTE-0002]]\n- [[FOO-NOTE-0001]]\n"
        );
        store.write_project_snapshot(&project, &[], &[]).unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("pwf-index.md")).unwrap(),
            "### Notes\n\n"
        );
        assert_eq!(fs::read(directory.path().join("foo.md")).unwrap(), page);
        assert_eq!(
            fs::read_to_string(directory.path().join("FOO-0001.md")).unwrap(),
            task
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("FOO-NOTE-0001.md")).unwrap(),
            note_body
        );
    }

    #[test]
    fn snapshot_creates_only_generated_page_in_a_missing_project_directory() {
        let directory = tempfile::tempdir().unwrap();
        let tasks = directory.path().join("tasks");
        let store = ObsidianStore::new(HomeDirectory::new(directory.path().to_path_buf()));
        store
            .write_project_snapshot(&project(&tasks), &[], &[])
            .unwrap();
        assert_eq!(
            fs::read_to_string(tasks.join("pwf-index.md")).unwrap(),
            "### Notes\n\n"
        );
        assert!(!tasks.join("foo.md").exists());
    }

    #[test]
    fn snapshot_failure_preserves_authored_page_tasks_and_notes() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObsidianStore::new(HomeDirectory::new(directory.path().to_path_buf()));
        let project = project(directory.path());
        let authored = [
            ("foo.md", "# My page\n"),
            ("FOO-0001.md", "---\nid: FOO-0001\n---\nTask body\n"),
            ("FOO-NOTE-0001.md", "# My note\n"),
        ];
        for (name, source) in authored {
            fs::write(directory.path().join(name), source).unwrap();
        }
        fs::create_dir(directory.path().join("pwf-index.md")).unwrap();
        let error = store
            .write_project_snapshot(
                &project,
                &[summary("FOO-0001", TaskStatus::Active)],
                &[note("FOO-NOTE-0001")],
            )
            .unwrap_err();
        assert_matches!(error, ObsidianStoreError::WriteProjectSnapshot { .. });
        for (name, source) in authored {
            assert_eq!(
                fs::read_to_string(directory.path().join(name)).unwrap(),
                source
            );
        }
        assert!(directory.path().join("pwf-index.md").is_dir());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 4);
    }

    #[test]
    fn snapshot_rejects_a_filename_reserved_for_the_authored_page() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObsidianStore::new(HomeDirectory::new(directory.path().to_path_buf()));
        let mut project = project(directory.path());
        project.title = ProjectName::try_new("pwf-index").unwrap();
        let page = b"# Authored project page\n";
        fs::write(directory.path().join("pwf-index.md"), page).unwrap();
        assert_matches!(
            store.write_project_snapshot(&project, &[], &[]),
            Err(ObsidianStoreError::ProjectPagePathReserved { .. })
        );
        assert_eq!(
            fs::read(directory.path().join("pwf-index.md")).unwrap(),
            page
        );
    }

    #[test]
    fn failed_note_read_leaves_the_previous_snapshot_and_authored_page_unchanged() {
        let directory = tempfile::tempdir().unwrap();
        let store = ObsidianStore::new(HomeDirectory::new(directory.path().to_path_buf()));
        let project = project(directory.path());
        fs::write(
            directory.path().join("FOO-0001.md"),
            "---\nid: FOO-0001\ntitle: Task\n---\nBody\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("FOO-NOTE-0001.md"),
            "---\ntype: note\n---\n",
        )
        .unwrap();
        let previous = "- [ ] [[FOO-0001]]\n\n### Notes\n\n- [[FOO-NOTE-0001]]\n";
        let page = "# Authored page\n";
        fs::write(directory.path().join("pwf-index.md"), previous).unwrap();
        fs::write(directory.path().join("foo.md"), page).unwrap();

        let result = refresh_project_snapshot::execute(&project, &store, &store, &store);

        assert_matches!(result, Err(pwf_application::project::refresh_project_snapshot::RefreshProjectSnapshotError::ReadNotes(_)));
        assert_eq!(
            fs::read_to_string(directory.path().join("pwf-index.md")).unwrap(),
            previous
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("foo.md")).unwrap(),
            page
        );
    }
}
