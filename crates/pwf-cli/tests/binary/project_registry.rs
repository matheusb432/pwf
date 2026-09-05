use crate::support::{
    ProjectFixture, add_payload, assert_failure, project_id, run_server_with_database,
};

#[test]
fn application_project_errors_are_emitted_without_command_prefixes() {
    let fixture = ProjectFixture::new().unwrap();

    for operation in ["get", "pause", "resume"] {
        let output = fixture.run(&["project", operation, "xyz"]).unwrap();

        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "Error: project not found: XYZ\n"
        );
    }
}

#[test]
fn server_database_open_failure_is_reported_only_as_a_diagnostic() {
    let directory = tempfile::tempdir().unwrap();

    assert_failure(
        run_server_with_database(directory.path()).unwrap(),
        &["project database", "unable to open database file"],
    )
    .unwrap();
}

#[test]
fn server_bootstraps_and_reopens_a_fresh_database() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("projects.sqlite3");
    for _ in 0..2 {
        let fixture = crate::support::DatabaseFixture::new(path.clone()).unwrap();
        let output = fixture.command().args(["project", "ls"]).output().unwrap();
        crate::support::assert_success(&output, "list fresh database");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
            serde_json::json!([])
        );
    }
    assert!(path.is_file());
}

#[test]
fn invalid_runtime_task_path_is_not_persisted() {
    let fixture = ProjectFixture::new().unwrap();
    let task_path = "~/tasks/../shared";
    let foo_project_id = project_id("foo").unwrap();
    let payload = add_payload(&foo_project_id, "foo", "/work/foo", task_path);

    assert_failure(
        fixture
            .run(&["project", "add", "--kind", "directory", &payload])
            .unwrap(),
        &["FOO", task_path],
    )
    .unwrap();
    assert_failure(fixture.run(&["project", "get", "FOO"]).unwrap(), &["FOO"]).unwrap();
}
