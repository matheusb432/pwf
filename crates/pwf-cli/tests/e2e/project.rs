use std::fs;

use crate::support::{
    ProjectFixture, assert_project, assert_success, project_id, success_json,
    write_project_rename_fixture,
};

#[test]
fn project_registry_lifecycle_is_observable_across_processes() {
    let fixture = ProjectFixture::new().unwrap();
    let bar_project_id = project_id("bar").unwrap();
    let foo_project_id = project_id("foo").unwrap();
    fixture
        .add(
            &bar_project_id,
            "bar-baz",
            "/work/bar-baz",
            "/pending-work/bar-baz",
        )
        .unwrap();
    let foo = fixture
        .add(
            &foo_project_id,
            "foo-bar",
            "/work/foo-bar",
            "/pending-work/foo-bar",
        )
        .unwrap();

    assert_eq!(
        success_json(fixture.run(&["project", "get", "foo"]).unwrap()).unwrap(),
        foo
    );
    let edited = fixture
        .run(&["project", "edit", "foo", "--source", "/work/foo-updated"])
        .unwrap();
    assert_success(&edited, "edit project source");
    assert_eq!(
        String::from_utf8(edited.stdout).unwrap(),
        "Edited project: FOO :: foo-bar\n"
    );
    assert!(edited.stderr.is_empty());
    let foo = success_json(fixture.run(&["project", "get", "foo"]).unwrap()).unwrap();
    assert_project(
        &foo,
        &foo_project_id,
        "foo-bar",
        "/work/foo-updated",
        "/pending-work/foo-bar",
        false,
    );
    assert_eq!(
        success_json(fixture.run(&["project", "pause", "bar", "--json"]).unwrap()).unwrap()["changed"],
        true
    );

    let projects = success_json(fixture.run(&["project", "ls", "--json"]).unwrap()).unwrap();
    let projects = projects.as_array().unwrap();
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
        "/work/foo-updated",
        "/pending-work/foo-bar",
        false,
    );

    assert_eq!(
        success_json(
            fixture
                .run(&["project", "resume", "bar", "--json"])
                .unwrap()
        )
        .unwrap()["changed"],
        true
    );
    assert_eq!(
        success_json(fixture.run(&["project", "get", "bar"]).unwrap()).unwrap()["is_paused"],
        false
    );
}

#[test]
fn project_rename_updates_registry_and_task_identity() {
    let fixture = ProjectFixture::new().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let source = directory.path().join("self/sample-app");
    let destination_source = directory.path().join("self/renamed-app");
    let tasks = directory.path().join("project-notes/self/sample-app");
    let destination_tasks = directory.path().join("project-notes/self/renamed-app");
    fs::create_dir_all(&source).unwrap();
    write_project_rename_fixture(&tasks).unwrap();
    let destination_project_id = project_id("NEW").unwrap();
    let created = fixture
        .add_with_home(
            &project_id("OLD").unwrap(),
            "sample-app",
            source.to_str().unwrap(),
            tasks.to_str().unwrap(),
            &home,
        )
        .unwrap();

    let renamed = success_json(
        fixture
            .run_with_home(
                &[
                    "project",
                    "rename",
                    "--json",
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
            )
            .unwrap(),
    )
    .unwrap();

    assert_project(
        &renamed,
        &destination_project_id,
        "renamed-app",
        destination_source.to_str().unwrap(),
        destination_tasks.to_str().unwrap(),
        false,
    );
    assert_eq!(renamed["created_at"], created["created_at"]);
    assert_eq!(renamed["snapshot_enabled"], created["snapshot_enabled"]);
    assert_eq!(
        success_json(fixture.run(&["project", "get", "NEW"]).unwrap()).unwrap(),
        renamed
    );

    let task = fixture.run(&["get", "NEW-0079"]).unwrap();
    assert!(task.status.success());
    let task = String::from_utf8(task.stdout).unwrap();
    assert!(task.contains("status: active"));
    assert!(task.contains("Task body remains intact."));
}
