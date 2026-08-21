use std::fs;

use crate::support::{
    ProjectFixture, assert_project, project_id, success_json, write_project_rename_fixture,
};

#[test]
fn project_registry_lifecycle_is_observable_across_processes() {
    let fixture = ProjectFixture::new();
    let bar_project_id = project_id("bar");
    let foo_project_id = project_id("foo");
    fixture.add(
        &bar_project_id,
        "bar-baz",
        "/work/bar-baz",
        "/pending-work/bar-baz",
    );
    let foo = fixture.add(
        &foo_project_id,
        "foo-bar",
        "/work/foo-bar",
        "/pending-work/foo-bar",
    );

    assert_eq!(success_json(fixture.run(&["project", "get", "foo"])), foo);
    assert_eq!(
        success_json(fixture.run(&["project", "pause", "bar"]))["changed"],
        true
    );

    let projects = success_json(fixture.run(&["project", "ls"]));
    let projects = projects.as_array().expect("list returns an array");
    assert_eq!(projects.len(), 2);
    assert_project(
        &projects[0],
        &bar_project_id,
        "bar-baz",
        "/work/bar-baz",
        "/pending-work/bar-baz",
        true,
    );
    assert_project(
        &projects[1],
        &foo_project_id,
        "foo-bar",
        "/work/foo-bar",
        "/pending-work/foo-bar",
        false,
    );

    assert_eq!(
        success_json(fixture.run(&["project", "resume", "bar"]))["changed"],
        true
    );
    assert_eq!(
        success_json(fixture.run(&["project", "get", "bar"]))["is_paused"],
        false
    );
}

#[test]
fn project_rename_updates_registry_and_task_identity() {
    let fixture = ProjectFixture::new();
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let source = directory.path().join("self/sample-app");
    let destination_source = directory.path().join("self/renamed-app");
    let tasks = directory.path().join("project-notes/self/sample-app");
    let destination_tasks = directory.path().join("project-notes/self/renamed-app");
    fs::create_dir_all(&source).unwrap();
    write_project_rename_fixture(&tasks);
    let destination_project_id = project_id("NEW");
    let created = fixture.add_with_home(
        &project_id("OLD"),
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

    let task = fixture.run(&["get", "NEW-0079"]);
    assert!(task.status.success());
    let task = String::from_utf8(task.stdout).unwrap();
    assert!(task.contains("status: active"));
    assert!(task.contains("Task body remains intact."));
}
