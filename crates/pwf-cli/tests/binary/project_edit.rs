use crate::support::{assert_failure, command};

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
