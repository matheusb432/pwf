use std::{fs, path::Path};

use crate::common::{ProjectFixture, assert_failure, assert_project, success_json};

fn write_fixture(tasks_path: &Path) {
    fs::create_dir_all(tasks_path).unwrap();
    fs::write(
        tasks_path.join("ssh-agent-phone-app.md"),
        "---\nid: ssh\ntitle: ssh-agent-phone-app\n---\n\n- [ ] [[SSH-0079]]\n",
    )
    .unwrap();
    fs::write(
        tasks_path.join("SSH-0079.md"),
        "---\nid: SSH-0079\nstatus: active\ntitle: keep body\nproject: ssh-agent-phone-app\ncreated: 2026-07-01\n---\n\nTask body remains intact.\n",
    )
    .unwrap();
}

#[test]
fn rename_commits_registry_and_task_identity_as_one_lifecycle() {
    let fixture = ProjectFixture::new();
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let source = directory.path().join("self/ssh-agent-phone-app");
    let destination_source = directory.path().join("self/mimux");
    let tasks = directory.path().join("pwf-db/self/ssh-agent-phone-app");
    let destination_tasks = directory.path().join("pwf-db/self/mimux");
    fs::create_dir_all(&source).unwrap();
    write_fixture(&tasks);
    let created = fixture.add_with_home(
        "SSH",
        "ssh-agent-phone-app",
        source.to_str().unwrap(),
        tasks.to_str().unwrap(),
        &home,
    );

    let renamed = success_json(fixture.run_with_home(
        &[
            "project",
            "rename",
            "SSH",
            "MUX",
            "--title",
            "mimux",
            "--source",
            destination_source.to_str().unwrap(),
            "--tasks",
            destination_tasks.to_str().unwrap(),
        ],
        &home,
    ));

    assert_project(
        &renamed,
        "MUX",
        "mimux",
        destination_source.to_str().unwrap(),
        destination_tasks.to_str().unwrap(),
        false,
    );
    assert_eq!(renamed["created_at"], created["created_at"]);
    assert_eq!(
        success_json(fixture.run(&["project", "get", "MUX"])),
        renamed
    );
    assert_failure(fixture.run(&["project", "get", "SSH"]), &["SSH"]);

    let shown = fixture.run(&["show", "MUX-0079"]);
    assert!(shown.status.success());
    let shown_stdout = String::from_utf8(shown.stdout).unwrap();
    assert!(shown_stdout.contains("status: active"));
    assert!(shown_stdout.contains("Task body remains intact."));
}

#[test]
fn rename_commit_failure_rolls_back_and_allows_retry() {
    let fixture = ProjectFixture::new();
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let source = directory.path().join("self/ssh-agent-phone-app");
    let destination_source = directory.path().join("self/mimux");
    let tasks = directory.path().join("pwf-db/self/ssh-agent-phone-app");
    let destination_tasks = directory.path().join("missing-parent/mimux");
    fs::create_dir_all(&source).unwrap();
    write_fixture(&tasks);
    let created = fixture.add_with_home(
        "SSH",
        "ssh-agent-phone-app",
        source.to_str().unwrap(),
        tasks.to_str().unwrap(),
        &home,
    );
    let rename_arguments = [
        "project",
        "rename",
        "SSH",
        "MUX",
        "--title",
        "mimux",
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
        success_json(fixture.run(&["project", "get", "SSH"])),
        created
    );
    assert_failure(fixture.run(&["project", "get", "MUX"]), &["MUX"]);
    assert!(fixture.run(&["show", "SSH-0079"]).status.success());

    fs::create_dir_all(destination_tasks.parent().unwrap()).unwrap();
    let retried = success_json(fixture.run_with_home(&rename_arguments, &home));

    assert_project(
        &retried,
        "MUX",
        "mimux",
        destination_source.to_str().unwrap(),
        destination_tasks.to_str().unwrap(),
        false,
    );
    assert!(fixture.run(&["show", "MUX-0079"]).status.success());
}
