use std::{fs, path::Path};

use pwf_application::project::refresh_project_snapshot::{self, RefreshProjectSnapshotError};
use pwf_infra::obsidian::ObsidianStore;
use pwf_models::project::{
    HomeDirectory, Project, ProjectId, ProjectName, ProjectTasks, ProjectTasksKind,
    ProjectTasksPath,
};
use tempfile::TempDir;

use super::fixture::{DocumentSize, document_body, require, temporary_directory};

pub const TASK_COUNT: usize = 1_000;
const NOTE_COUNT: usize = 100;
const AUTHORED_PAGE: &str = "# Project notes\n\nKeep this authored page unchanged.\n";

pub struct SnapshotWorkload {
    directory: TempDir,
    store: ObsidianStore,
    project: Project,
}

impl SnapshotWorkload {
    pub fn new() -> Self {
        let directory = temporary_directory("snapshot-refresh-");
        let tasks_path = directory.path().join("tasks");
        write_fixture(&tasks_path);
        let project = Project {
            id: require(
                ProjectId::try_new("FOO"),
                "constructing snapshot project ID",
            ),
            title: require(
                ProjectName::try_new("foo"),
                "constructing snapshot project title",
            ),
            source: None,
            tasks: ProjectTasks::new(
                ProjectTasksKind::Directory,
                require(
                    ProjectTasksPath::try_new(tasks_path.to_string_lossy()),
                    "constructing snapshot task path",
                ),
            ),
            obsidian_vault: None,
            created_at: require(
                "2026-09-10T00:00:00Z".parse(),
                "constructing snapshot project timestamp",
            ),
            is_paused: false,
            snapshot_enabled: true,
        };
        Self {
            directory,
            store: ObsidianStore::new(HomeDirectory::new(tasks_path)),
            project,
        }
    }

    pub fn refresh(&self) -> Result<(), RefreshProjectSnapshotError> {
        refresh_project_snapshot::execute(&self.project, &self.store, &self.store, &self.store)
    }

    pub fn validate(&self) {
        let path = self.directory.path().join("tasks");
        let generated = require(
            fs::read_to_string(path.join("pwf-index.md")),
            "reading generated snapshot",
        );
        assert_eq!(
            generated
                .lines()
                .filter(|line| line.starts_with("- ["))
                .count(),
            TASK_COUNT + NOTE_COUNT
        );
        assert_eq!(
            generated
                .lines()
                .filter(|line| line.starts_with("- [ ]"))
                .count(),
            600
        );
        assert_eq!(
            generated
                .lines()
                .filter(|line| line.starts_with("- [x]"))
                .count(),
            400
        );
        assert!(generated.starts_with("- [ ] [[FOO-1000]]\n- [x] [[FOO-0999]]\n"));
        assert!(generated.contains("### Notes\n\n- [[FOO-NOTE-0100]]\n"));
        assert!(generated.ends_with("- [[FOO-NOTE-0001]]\n"));
        assert_eq!(
            require(
                fs::read_to_string(path.join("foo.md")),
                "reading authored page"
            ),
            AUTHORED_PAGE
        );
    }
}

fn write_fixture(path: &Path) {
    require(fs::create_dir_all(path), "creating snapshot fixture");
    require(
        fs::write(path.join("foo.md"), AUTHORED_PAGE),
        "writing authored page",
    );
    let body = document_body(DocumentSize::Small).replace("PWF-0001", "FOO-0001");
    for number in 1..=TASK_COUNT {
        let (status, completion) = match number % 10 {
            0..=5 => ("active", ""),
            6..=8 => ("done", "completed_at: 2026-09-09T12:34:56Z\n"),
            _ => ("cancelled", "completed_at: 2026-09-09T12:34:56Z\n"),
        };
        let source = format!(
            "---\nid: FOO-{number:04}\ntitle: Snapshot task {number:04}\nstatus: {status}\nproject: foo\ncreated_at: 2026-09-01T12:34:56Z\n{completion}effort: medium\npriority: high\ntags: [benchmark, obsidian]\nblocked_by: [\"[[AUX-0001]]\"]\n---\n\n{body}"
        );
        require(
            fs::write(path.join(format!("FOO-{number:04}.md")), source),
            "writing snapshot task",
        );
    }
    for number in 1..=NOTE_COUNT {
        let source = format!(
            "---\ntype: note\nproject: foo\ncreated: 2026-09-01\ntags: [benchmark, notes]\nverified: 2026-09-09\n---\n\n# Snapshot note {number:04}\n\n{body}"
        );
        require(
            fs::write(path.join(format!("FOO-NOTE-{number:04}.md")), source),
            "writing snapshot note",
        );
    }
}
