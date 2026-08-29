use std::fs;

use crate::support::{DatabaseFixture, project_id};

#[test]
fn check_is_read_only_and_apply_converges_task_metadata() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let database = DatabaseFixture::new(directory.path().join("projects.sqlite3"))?;
    let project_path = directory.path().join("project");
    let tasks_path = directory.path().join("tasks");
    fs::create_dir_all(&project_path)?;
    fs::create_dir_all(&tasks_path)?;
    let index_path = tasks_path.join("foo.md");
    let task_path = tasks_path.join("FOO-0001.md");
    let index = "---\nid: FOO\ntitle: foo\n---\n\n- [x] [[FOO-0001|done task]] ✅ 2026-07-03\n";
    let task = "---\nid: FOO-0001\nstatus: done\ntitle: done task\nproject: foo\ncreated: 2026-07-01\ncompleted: 2026-07-02\n---\n\nbody\n";
    fs::write(&index_path, index)?;
    fs::write(&task_path, task)?;
    database.add_directory_project(&project_id("FOO")?, "foo", &project_path, &tasks_path);

    let preview = database.migrator_command().arg("--check").output()?;

    assert!(!preview.status.success());
    assert!(String::from_utf8(preview.stdout)?.contains("1 task file change(s)"));
    assert!(String::from_utf8(preview.stderr)?.contains("migration is pending"));
    assert_eq!(fs::read_to_string(&task_path)?, task);
    assert_eq!(fs::read_to_string(&index_path)?, index);

    let applied = database.migrator_command().output()?;

    assert!(
        applied.status.success(),
        "migrator failed: {}",
        String::from_utf8_lossy(&applied.stderr)
    );
    assert_eq!(
        fs::read_to_string(&task_path)?,
        "---\nid: FOO-0001\nstatus: done\ntitle: done task\nproject: foo\ncreated_at: 2026-07-01T00:00:00Z\ncompleted_at: 2026-07-02T00:00:00Z\n---\n\nbody\n"
    );
    assert_eq!(
        fs::read_to_string(&index_path)?,
        index.replace(" ✅ 2026-07-03", "")
    );

    let current = database.migrator_command().arg("--check").output()?;
    assert!(
        current.status.success(),
        "check failed: {}",
        String::from_utf8_lossy(&current.stderr)
    );
    assert!(String::from_utf8(current.stdout)?.contains("0 task file change(s)"));
    Ok(())
}
