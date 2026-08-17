use std::{fs, path::Path};

use crate::shared::{ProjectFixture, assert_failure, assert_project, project_id, success_json};

fn write_fixture(tasks_path: &Path) {
    fs::create_dir_all(tasks_path).unwrap();
    fs::write(
        tasks_path.join("sample-app.md"),
        "---\nid: old\ntitle: sample-app\n---\n\n- [ ] [[OLD-0079]]\n",
    )
    .unwrap();
    fs::write(
        tasks_path.join("OLD-0079.md"),
        "---\nid: OLD-0079\nstatus: active\ntitle: keep body\nproject: sample-app\ncreated: 2026-07-01\n---\n\nTask body remains intact.\n",
    )
    .unwrap();
}

#[test]
fn rename_commits_registry_and_task_identity_as_one_lifecycle() {
    let fixture = ProjectFixture::new();
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let source = directory.path().join("self/sample-app");
    let destination_source = directory.path().join("self/renamed-app");
    let tasks = directory.path().join("project-notes/self/sample-app");
    let destination_tasks = directory.path().join("project-notes/self/renamed-app");
    fs::create_dir_all(&source).unwrap();
    write_fixture(&tasks);
    let source_project_id = project_id("OLD");
    let destination_project_id = project_id("NEW");
    let created = fixture.add_with_home(
        &source_project_id,
        "sample-app",
        source.to_str().unwrap(),
        tasks.to_str().unwrap(),
        &home,
    );

    let renamed = success_json(fixture.run_with_home(
        &[
            "project",
            "rename",
            "OLD",
            "NEW",
            "--title",
            "renamed-app",
            "--source",
            destination_source.to_str().unwrap(),
            "--tasks",
            destination_tasks.to_str().unwrap(),
        ],
        &home,
    ));

    assert_project(
        &renamed,
        &destination_project_id,
        "renamed-app",
        destination_source.to_str().unwrap(),
        destination_tasks.to_str().unwrap(),
        false,
    );
    assert_eq!(renamed["created_at"], created["created_at"]);
    assert_eq!(
        success_json(fixture.run(&["project", "get", "NEW"])),
        renamed
    );
    assert_failure(fixture.run(&["project", "get", "OLD"]), &["OLD"]);

    let gotten = fixture.run(&["get", "NEW-0079"]);
    assert!(gotten.status.success());
    let gotten_stdout = String::from_utf8(gotten.stdout).unwrap();
    assert!(gotten_stdout.contains("status: active"));
    assert!(gotten_stdout.contains("Task body remains intact."));
}

#[test]
fn rename_commit_failure_rolls_back_and_allows_retry() {
    let fixture = ProjectFixture::new();
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let source = directory.path().join("self/sample-app");
    let destination_source = directory.path().join("self/renamed-app");
    let tasks = directory.path().join("project-notes/self/sample-app");
    let destination_tasks = directory.path().join("missing-parent/renamed-app");
    fs::create_dir_all(&source).unwrap();
    write_fixture(&tasks);
    let source_project_id = project_id("OLD");
    let destination_project_id = project_id("NEW");
    let created = fixture.add_with_home(
        &source_project_id,
        "sample-app",
        source.to_str().unwrap(),
        tasks.to_str().unwrap(),
        &home,
    );
    let rename_arguments = [
        "project",
        "rename",
        "OLD",
        destination_project_id.as_ref(),
        "--title",
        "renamed-app",
        "--source",
        destination_source.to_str().unwrap(),
        "--tasks",
        destination_tasks.to_str().unwrap(),
    ];

    assert_failure(
        fixture.run_with_home(&rename_arguments, &home),
        &["rename", "rollback"],
    );
    assert_eq!(
        success_json(fixture.run(&["project", "get", "OLD"])),
        created
    );
    assert_failure(fixture.run(&["project", "get", "NEW"]), &["NEW"]);
    assert!(fixture.run(&["get", "OLD-0079"]).status.success());

    fs::create_dir_all(destination_tasks.parent().unwrap()).unwrap();
    let retried = success_json(fixture.run_with_home(&rename_arguments, &home));

    assert_project(
        &retried,
        &destination_project_id,
        "renamed-app",
        destination_source.to_str().unwrap(),
        destination_tasks.to_str().unwrap(),
        false,
    );
    assert!(fixture.run(&["get", "NEW-0079"]).status.success());
}
