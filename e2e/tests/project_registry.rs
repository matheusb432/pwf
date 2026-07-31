use crate::shared::{
    ProjectFixture, add_payload, assert_failure, assert_project, run_with_database, success_json,
};

#[test]
fn registry_lifecycle_is_observable_across_processes() {
    let fixture = ProjectFixture::new();
    let bar = fixture.add("bar", "bar-baz", "/work/bar-baz", "/pending-work/bar-baz");
    let foo = fixture.add("foo", "foo-bar", "/work/foo-bar", "/pending-work/foo-bar");

    assert_eq!(success_json(fixture.run(&["project", "get", "foo"])), foo);

    let paused = success_json(fixture.run(&["project", "pause", "bar"]));
    assert_eq!(paused["changed"], true);
    assert_eq!(
        success_json(fixture.run(&["project", "pause", "bar"]))["changed"],
        false
    );

    let projects = success_json(fixture.run(&["project", "ls"]));
    let projects = projects.as_array().expect("list returns an array");
    assert_eq!(projects.len(), 2);
    assert_project(
        &projects[0],
        "BAR",
        "bar-baz",
        "/work/bar-baz",
        "/pending-work/bar-baz",
        true,
    );
    assert_project(
        &projects[1],
        "FOO",
        "foo-bar",
        "/work/foo-bar",
        "/pending-work/foo-bar",
        false,
    );

    let resumed = success_json(fixture.run(&["project", "resume", "bar"]));
    assert_eq!(resumed["changed"], true);
    assert_eq!(
        success_json(fixture.run(&["project", "resume", "bar"]))["changed"],
        false
    );
    assert_eq!(success_json(fixture.run(&["project", "get", "bar"])), bar);
}

#[test]
fn unknown_project_operations_fail_without_stdout() {
    let fixture = ProjectFixture::new();

    for operation in ["get", "pause", "resume"] {
        assert_failure(fixture.run(&["project", operation, "xyz"]), &["XYZ"]);
    }
}

#[test]
fn database_open_failure_is_reported_only_as_a_diagnostic() {
    let directory = tempfile::tempdir().unwrap();

    assert_failure(
        run_with_database(directory.path(), &["project", "ls"]),
        &["project database"],
    );
}

#[test]
fn invalid_runtime_task_path_is_not_persisted() {
    let fixture = ProjectFixture::new();
    let task_path = "~/tasks/../shared";
    let payload = add_payload("pwf", "pwf", "/work/pwf", task_path);

    assert_failure(
        fixture.run(&["project", "add", "--kind", "directory", &payload]),
        &["PWF", task_path],
    );
    assert_failure(fixture.run(&["project", "get", "PWF"]), &["PWF"]);
}

#[test]
fn pending_work_uses_only_valid_active_project_mappings() {
    let unknown = ProjectFixture::new();
    assert_failure(unknown.run(&["missing"]), &["missing"]);

    let paused = ProjectFixture::new();
    paused.add("pwf", "pwf", "/work/pwf", "/pending-work/pwf");
    success_json(paused.run(&["project", "pause", "pwf"]));

    assert_failure(paused.run(&["pwf"]), &["pwf"]);

    let aliased = ProjectFixture::new();
    let directory = tempfile::tempdir().unwrap();
    let seed_home = directory.path().join("seed-home");
    let runtime_home = directory.path().join("runtime-home");
    let absolute_tasks_path = runtime_home
        .join("missing/tasks/shared")
        .to_string_lossy()
        .into_owned();
    aliased.add_with_home(
        "pwf",
        "pwf",
        "/work/pwf",
        "~/missing/tasks/shared",
        &seed_home,
    );
    aliased.add_with_home(
        "alt",
        "other",
        "/work/other",
        &absolute_tasks_path,
        &seed_home,
    );

    assert_failure(
        aliased.run_with_home(&["list", "--project", "pwf"], &runtime_home),
        &["ALT", "PWF", &absolute_tasks_path],
    );
}
