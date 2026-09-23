use std::path::{Path, PathBuf};

use pwf_application::{ports::task_source_file::TaskSourceFileReader, task::add_task_from_file};
use pwf_wire::task::AddTaskFromFile;

use crate::support::{FixedClock, InMemoryStore, insert_project};

#[derive(Clone)]
struct SourceFileReader {
    expected_path: PathBuf,
    body: String,
}

impl TaskSourceFileReader for SourceFileReader {
    fn read_task_source_file(&self, path: &Path) -> std::io::Result<String> {
        assert_eq!(path, self.expected_path);
        Ok(self.body.clone())
    }
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn add_from_file_uses_the_filename_and_authored_body(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store = InMemoryStore::default().with_project_id("foo", "FOO");
    let source_file = PathBuf::from("/tmp/My very cool task.md");
    let body = "# This is important\n\nKeep /g exactly as authored.\n";

    let added = add_task_from_file::execute(
        AddTaskFromFile {
            project_id: "FOO".parse().unwrap(),
            source_file: source_file.clone(),
        },
        &SourceFileReader {
            expected_path: source_file,
            body: body.to_string(),
        },
        &store,
        &pwf_infra::project_store::SqliteProjectStore::new(pool.clone()),
        &FixedClock,
        &pwf_infra::task_marker_section_store::SqliteTaskMarkerSectionStore::new(pool),
    )
    .await
    .unwrap();

    assert_eq!(added.outcome.as_ref(), "FOO-0001");
    assert_eq!(added.task.as_ref().unwrap().title, "My very cool task");
    assert_eq!(store.tasks("foo")[0].title, "My very cool task");
    assert_eq!(store.tasks("foo")[0].body, body);
}
