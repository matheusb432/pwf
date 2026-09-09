#[cfg(unix)]
use crate::support::style::color_rgb;
use crate::support::{
    CommandTestExt, DatabaseFixture, ManagedProject, ProjectFixture, add_payload, assert_failure,
    project_id, run_server_with_database,
};

#[test]
fn project_list_defaults_to_rows_and_preserves_json_through_both_names() {
    let fixture = ManagedProject::new(&project_id("PWF").unwrap(), "pwf").unwrap();
    for name in ["list", "ls"] {
        let output = fixture
            .database
            .command_args(&["project", name])
            .success_stdout();
        assert_eq!(output, "PWF :: pwf\n");
        let json = fixture
            .database
            .command_args(&["project", name, "--json"])
            .success_json();
        assert_eq!(json[0]["id"], "PWF");
        assert_eq!(json[0]["title"], "pwf");
        assert_eq!(json[0]["is_paused"], false);
        assert!(json[0].get("tasks").is_some());
    }
}

#[test]
#[cfg(unix)]
fn project_list_colors_only_identifiers_by_project_state() {
    let fixture = ManagedProject::new(&project_id("PWF").unwrap(), "pwf").unwrap();
    let configured = "[colors.project]\nactive = \"#010203\"\npaused = \"#040506\"\n";
    for (config, state, color) in [
        ("", "resume", color_rgb(100, 149, 237)),
        ("", "pause", color_rgb(255, 107, 138)),
        (configured, "resume", color_rgb(1, 2, 3)),
        (configured, "pause", color_rgb(4, 5, 6)),
    ] {
        fixture.database.write_user_config(config).unwrap();
        fixture
            .database
            .command_args(&["project", state, "PWF"])
            .success_json();
        let output = fixture
            .database
            .command_args(&["project", "list"])
            .color()
            .success_stdout();
        assert_eq!(output, format!("{color}PWF{color:#} :: pwf\n"));
        let json = fixture
            .database
            .command_args(&["project", "list", "--json"])
            .color()
            .success_json();
        assert_eq!(json[0]["is_paused"], state == "pause");
    }
}

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
        let fixture = DatabaseFixture::new(path.clone()).unwrap();
        assert_eq!(
            fixture.command_args(&["project", "list"]).success_stdout(),
            ""
        );
        assert_eq!(
            fixture
                .command_args(&["project", "ls", "--json"])
                .success_json(),
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

#[test]
fn add_vault_uses_current_directory_and_source_less_projects_support_tasks() {
    let root = tempfile::tempdir().unwrap();
    let vault = root.path().join("vault");
    std::fs::create_dir_all(vault.join(".obsidian")).unwrap();
    let fixture = DatabaseFixture::new(root.path().join("pwf.sqlite3")).unwrap();
    let created = fixture
        .command_args(&[
            "project",
            "add-vault",
            "--id",
            "foo",
            "--tasks-path",
            "tasks/foo",
        ])
        .current_dir(&vault)
        .success_json();
    assert_eq!(created, serde_json::json!({"id": "FOO"}));
    let project = fixture
        .command_args(&["project", "get", "foo"])
        .success_json();
    assert_eq!(project["title"], "foo");
    assert!(project["source"].is_null());
    fixture
        .command_args(&[
            "task",
            "add",
            "foo",
            "--title",
            "sample",
            "--goal",
            "verify a vault project",
        ])
        .success_stdout();
    let listed = fixture
        .command_args(&["task", "list", "--project", "foo", "--long"])
        .success_stdout();
    assert!(listed.contains("project_path: none"));
    assert_failure(
        fixture
            .command_args(&["session", "foo1", "--dry-run"])
            .output()
            .unwrap(),
        &["has no source path", "pwf project edit FOO --source"],
    )
    .unwrap();
    fixture
        .command_args(&["project", "edit", "foo", "--source"])
        .arg(root.path())
        .success_stdout();
    let sourced = fixture
        .command_args(&["project", "get", "foo"])
        .success_json();
    assert_eq!(sourced["source"]["value"], root.path().to_str().unwrap());
    fixture
        .command_args(&["project", "edit", "foo", "--clear-source"])
        .success_stdout();
    let projects = fixture
        .command_args(&["project", "ls", "--json"])
        .success_json();
    assert_eq!(projects.as_array().unwrap().len(), 1);
    assert!(projects[0]["source"].is_null());
}
