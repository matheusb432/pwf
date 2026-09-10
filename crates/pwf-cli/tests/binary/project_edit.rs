use crate::support::{assert_failure, command};

#[test]
fn project_snapshot_edit_requires_a_boolean_and_preserves_omitted_values() {
    use crate::support::{CommandTestExt, ManagedProject, project_id};

    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo").unwrap();
    let before = fixture
        .database
        .command_args(&["project", "get", "FOO"])
        .success_json();
    assert_eq!(before["snapshot_enabled"], false);

    for arguments in [
        vec!["project", "edit", "FOO", "--snapshot-enabled"],
        vec!["project", "edit", "FOO", "--snapshot-enabled", "yes"],
    ] {
        let output = fixture.database.command_args(&arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert_failure(output, &["--snapshot-enabled", "true", "false"]).unwrap();
    }
    assert_eq!(
        fixture
            .database
            .command_args(&["project", "get", "FOO"])
            .success_json(),
        before
    );

    for enabled in [true, false] {
        assert_eq!(
            fixture
                .database
                .command_args(&[
                    "project",
                    "edit",
                    "FOO",
                    "--snapshot-enabled",
                    if enabled { "true" } else { "false" },
                ])
                .success_stdout(),
            ""
        );
        let mut expected = before.clone();
        expected["snapshot_enabled"] = enabled.into();
        assert_eq!(
            fixture
                .database
                .command_args(&["project", "get", "FOO"])
                .success_json(),
            expected
        );

        fixture
            .database
            .command_args(&["project", "edit", "FOO", "--obsidian-vault", "~/notes"])
            .success_stdout();
        fixture
            .database
            .command_args(&["project", "edit", "FOO", "--clear-obsidian-vault"])
            .success_stdout();
        let listed = fixture
            .database
            .command_args(&["project", "list", "--json"])
            .success_json();
        assert_eq!(listed[0], expected);
    }
}

#[test]
fn project_edit_requires_at_least_one_update() {
    let output = command().args(["project", "edit", "foo"]).output().unwrap();

    assert_failure(output, &["--source <SOURCE>"]).unwrap();
}

#[test]
fn project_vault_json_and_independent_edit_clear_round_trip() {
    use assert_cmd::prelude::OutputAssertExt as _;

    use crate::support::{ManagedProject, project_id, success_json};
    let fixture = ManagedProject::new(&project_id("FOO").unwrap(), "foo").unwrap();
    let before = success_json(
        fixture
            .database
            .command()
            .args(["project", "get", "FOO"])
            .output()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(before.get("obsidian_vault"), Some(&serde_json::Value::Null));
    fixture
        .database
        .command()
        .args(["project", "edit", "FOO", "--obsidian-vault", "~/notes"])
        .assert()
        .success();
    let configured = success_json(
        fixture
            .database
            .command()
            .args(["project", "get", "FOO"])
            .output()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(configured["obsidian_vault"], "~/notes");
    assert_eq!(configured["source"], before["source"]);
    fixture
        .database
        .command()
        .args(["project", "edit", "FOO", "--clear-obsidian-vault"])
        .assert()
        .success();
    let cleared = success_json(
        fixture
            .database
            .command()
            .args(["project", "get", "FOO"])
            .output()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(cleared, before);
}
