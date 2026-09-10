use std::fs;

use crate::support::{
    ProjectFixture, assert_failure, assert_project, project_id, success_json,
    write_project_rename_fixture,
};

#[test]
fn rename_commit_failure_rolls_back_and_allows_retry() {
    let fixture = ProjectFixture::new().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let source = directory.path().join("self/sample-app");
    let destination_source = directory.path().join("self/renamed-app");
    let tasks = directory.path().join("project-notes/self/sample-app");
    let destination_tasks = directory.path().join("missing-parent/renamed-app");
    fs::create_dir_all(&source).unwrap();
    write_project_rename_fixture(&tasks).unwrap();
    let source_project_id = project_id("OLD").unwrap();
    let destination_project_id = project_id("NEW").unwrap();
    fixture
        .add_with_home(
            &source_project_id,
            "sample-app",
            source.to_str().unwrap(),
            tasks.to_str().unwrap(),
            &home,
        )
        .unwrap();
    let enabled = fixture
        .run(&["project", "edit", "OLD", "--snapshot-enabled", "true"])
        .unwrap();
    assert!(enabled.status.success());
    let created = success_json(fixture.run(&["project", "get", "OLD"]).unwrap()).unwrap();
    assert_eq!(created["snapshot_enabled"], true);
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
        fixture.run_with_home(&rename_arguments, &home).unwrap(),
        &["rename", "rollback"],
    )
    .unwrap();
    assert_eq!(
        success_json(fixture.run(&["project", "get", "OLD"]).unwrap()).unwrap(),
        created
    );
    assert_failure(fixture.run(&["project", "get", "NEW"]).unwrap(), &["NEW"]).unwrap();
    assert!(fixture.run(&["get", "OLD-0079"]).unwrap().status.success());

    fs::create_dir_all(destination_tasks.parent().unwrap()).unwrap();
    let retried = success_json(fixture.run_with_home(&rename_arguments, &home).unwrap()).unwrap();
    assert_eq!(retried["snapshot_enabled"], true);

    assert_project(
        &retried,
        &destination_project_id,
        "renamed-app",
        destination_source.to_str().unwrap(),
        destination_tasks.to_str().unwrap(),
        false,
    );
    assert!(fixture.run(&["get", "NEW-0079"]).unwrap().status.success());
}
