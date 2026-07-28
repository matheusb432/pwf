//! Checks managed-project commands through the real pwf process.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::{Value, json};
use tempfile::TempDir;

struct ProjectCli {
    _directory: TempDir,
    database_path: PathBuf,
}

impl ProjectCli {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("create project CLI test directory");
        let database_path = directory.path().join("projects.sqlite3");
        Self {
            _directory: directory,
            database_path,
        }
    }

    fn run(&self, arguments: &[&str]) -> Output {
        self.run_with_home(arguments, None)
    }

    fn run_with_home(&self, arguments: &[&str], home: Option<&Path>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pwf"));
        command
            .args(arguments)
            .env("PWF_DATABASE_PATH", &self.database_path);
        if let Some(home) = home {
            command.env("HOME", home);
        }
        command.output().expect("run pwf")
    }

    fn add_with_home(
        &self,
        id: &str,
        title: &str,
        source: &str,
        tasks: &str,
        home: &Path,
    ) -> Value {
        let payload = add_payload(id, title, source, tasks);
        success_json(self.run_with_home(
            &["project", "add", "--kind", "directory", &payload],
            Some(home),
        ))
    }

    fn database_path(&self) -> &Path {
        &self.database_path
    }
}

fn add_payload(id: &str, title: &str, source: &str, tasks: &str) -> String {
    json!({
        "id": id,
        "title": title,
        "source": {
            "value": source,
        },
        "tasks": {
            "kind": "directory",
            "path": tasks,
        },
    })
    .to_string()
}

fn add(cli: &ProjectCli, id: &str, title: &str, source: &str, tasks: &str) -> Value {
    let payload = add_payload(id, title, source, tasks);
    let output = cli.run(&["project", "add", "--kind", "directory", &payload]);
    success_json(output)
}

fn success_json(output: Output) -> Value {
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        output.status.success(),
        "expected success\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stderr.is_empty(), "successful command stderr: {stderr}");

    let value: Value = serde_json::from_str(&stdout).expect("stdout is JSON");
    assert!(
        stdout.starts_with("{\n  \"") || stdout.starts_with("[\n  {"),
        "stdout should use multi-line two-space JSON indentation: {stdout}"
    );
    assert!(
        stdout.ends_with('\n'),
        "stdout should end with one line break: {stdout}"
    );
    value
}

fn assert_project(
    project: &Value,
    id: &str,
    title: &str,
    source: &str,
    tasks: &str,
    is_paused: bool,
) {
    assert_eq!(project["id"], id);
    assert_eq!(project["title"], title);
    assert_eq!(
        project["source"],
        json!({"kind": "directory", "value": source})
    );
    assert_eq!(
        project["tasks"],
        json!({"kind": "directory", "path": tasks})
    );
    assert!(
        project["created_at"]
            .as_str()
            .is_some_and(|created_at| created_at.ends_with('Z')),
        "created_at should be a UTC timestamp: {project}"
    );
    assert_eq!(project["is_paused"], is_paused);
    assert_eq!(
        project.as_object().map(serde_json::Map::len),
        Some(6),
        "project output should contain only the approved fields"
    );
}

fn assert_failure(output: Output, diagnostic: &[&str]) {
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(!output.status.success(), "command should fail: {stdout}");
    assert!(stdout.is_empty(), "failed command stdout: {stdout}");
    for fragment in diagnostic {
        assert!(
            stderr.contains(fragment),
            "stderr should contain {fragment:?}: {stderr}"
        );
    }
}

fn run_with_database(database_path: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pwf"))
        .args(arguments)
        .env("PWF_DATABASE_PATH", database_path)
        .output()
        .expect("run pwf")
}

#[test]
fn add_creates_database_and_returns_the_project() {
    let cli = ProjectCli::new();

    let project = add(&cli, "pwf", "pwf", "/work/pwf", "/pending-work/pwf");

    assert!(cli.database_path().is_file());
    assert_project(
        &project,
        "PWF",
        "pwf",
        "/work/pwf",
        "/pending-work/pwf",
        false,
    );
}

#[test]
fn get_reads_a_project_created_by_another_process() {
    let cli = ProjectCli::new();
    let created = add(
        &cli,
        "foo",
        "foo-bar",
        "/work/foo-bar",
        "/pending-work/foo-bar",
    );

    let fetched = success_json(cli.run(&["project", "get", "foo"]));

    assert_eq!(fetched, created);
}

#[test]
fn list_is_title_sorted_and_includes_paused_projects() {
    let cli = ProjectCli::new();
    add(
        &cli,
        "bar",
        "bar-baz",
        "/work/bar-baz",
        "/pending-work/bar-baz",
    );
    add(
        &cli,
        "foo",
        "foo-bar",
        "/work/foo-bar",
        "/pending-work/foo-bar",
    );
    success_json(cli.run(&["project", "pause", "bar"]));

    let projects = success_json(cli.run(&["project", "ls"]));
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
}

#[test]
fn pause_and_resume_report_changes_and_repeated_no_ops() {
    let cli = ProjectCli::new();
    add(&cli, "pwf", "pwf", "/work/pwf", "/pending-work/pwf");

    let paused = success_json(cli.run(&["project", "pause", "pwf"]));
    assert_eq!(paused["changed"], true);
    assert_project(
        &paused["project"],
        "PWF",
        "pwf",
        "/work/pwf",
        "/pending-work/pwf",
        true,
    );

    let paused_again = success_json(cli.run(&["project", "pause", "pwf"]));
    assert_eq!(paused_again["changed"], false);
    assert_eq!(paused_again["project"], paused["project"]);

    let resumed = success_json(cli.run(&["project", "resume", "pwf"]));
    assert_eq!(resumed["changed"], true);
    assert_project(
        &resumed["project"],
        "PWF",
        "pwf",
        "/work/pwf",
        "/pending-work/pwf",
        false,
    );

    let resumed_again = success_json(cli.run(&["project", "resume", "pwf"]));
    assert_eq!(resumed_again["changed"], false);
    assert_eq!(resumed_again["project"], resumed["project"]);
}

#[test]
fn unknown_project_ids_fail_without_stdout() {
    for leaf in ["get", "pause", "resume"] {
        let cli = ProjectCli::new();
        let output = cli.run(&["project", leaf, "xyz"]);

        assert_failure(output, &["Error:", "project not found: XYZ"]);
    }
}

#[test]
fn existing_engines_reject_unknown_and_paused_projects() {
    let unknown = ProjectCli::new();
    assert_failure(
        unknown.run(&["missing"]),
        &["Unknown managed project identifier: missing"],
    );

    let paused = ProjectCli::new();
    add(&paused, "pwf", "pwf", "/work/pwf", "/pending-work/pwf");
    success_json(paused.run(&["project", "pause", "pwf"]));

    assert_failure(
        paused.run(&["pwf"]),
        &["Unknown managed project identifier: pwf"],
    );
}

#[test]
fn malformed_add_json_fails_before_database_creation() {
    let cli = ProjectCli::new();

    let output = cli.run(&["project", "add", "--kind", "directory", r#"{"id":"PWF""#]);

    assert_failure(output, &["project", "JSON"]);
    assert!(!cli.database_path().exists());
}

#[test]
fn duplicate_add_fields_fail_before_database_creation() {
    let cli = ProjectCli::new();
    let payload = r#"{
        "id": "PWF",
        "id": "ARC",
        "title": "pwf",
        "source": {"value": "/work/pwf"},
        "tasks": {"kind": "directory", "path": "/pending-work/pwf"}
    }"#;

    let output = cli.run(&["project", "add", "--kind", "directory", payload]);

    assert_failure(output, &["project", "duplicate field", "id"]);
    assert!(!cli.database_path().exists());
}

#[test]
fn database_path_failures_do_not_write_stdout() {
    let directory = tempfile::tempdir().unwrap();

    let output = run_with_database(directory.path(), &["project", "ls"]);

    assert_failure(output, &["Error:", "opening project database"]);
}

#[test]
fn reserved_project_title_fails_before_database_creation() {
    let cli = ProjectCli::new();
    let payload = add_payload("pwf", "project", "/work/pwf", "/pending-work/pwf");

    let output = cli.run(&["project", "add", "--kind", "directory", &payload]);

    assert_failure(output, &["project", "title"]);
    assert!(!cli.database_path().exists());
}

#[test]
fn unsupported_source_kind_fails_at_clap_before_database_creation() {
    let cli = ProjectCli::new();
    let payload = add_payload("pwf", "pwf", "/work/pwf", "/pending-work/pwf");

    let output = cli.run(&["project", "add", "--kind", "remote", &payload]);

    assert_failure(output, &["remote", "directory"]);
    assert!(!cli.database_path().exists());
}

#[test]
fn unsupported_tasks_kind_fails_at_json_parsing_before_database_creation() {
    let cli = ProjectCli::new();
    let payload = json!({
        "id": "pwf",
        "title": "pwf",
        "source": {
            "value": "/work/pwf",
        },
        "tasks": {
            "kind": "remote",
            "path": "/pending-work/pwf",
        },
    })
    .to_string();

    let output = cli.run(&["project", "add", "--kind", "directory", &payload]);

    assert_failure(output, &["project", "remote", "directory"]);
    assert!(!cli.database_path().exists());
}

#[test]
fn malformed_project_task_paths_fail_before_database_creation() {
    for tasks_path in [
        "~//tmp/tasks",
        r"~\\tmp\tasks",
        r"~/\tmp/tasks",
        "~/tasks/../shared",
        "~/D:/tasks",
        "~/D:tasks",
    ] {
        let cli = ProjectCli::new();
        let payload = add_payload("pwf", "pwf", "/work/pwf", tasks_path);

        let output = cli.run(&["project", "add", "--kind", "directory", &payload]);

        assert_failure(
            output,
            &["managed project PWF task path", "is invalid", tasks_path],
        );
        assert!(
            !cli.database_path().exists(),
            "invalid path created the project database: {tasks_path:?}"
        );
    }
}

#[test]
fn add_rejects_a_runtime_task_path_owned_by_a_paused_project() {
    let cli = ProjectCli::new();
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let absolute_tasks_path = home.join("tasks/shared").to_string_lossy().into_owned();
    let existing = cli.add_with_home("pwf", "pwf", "/work/pwf", "~/tasks/shared", &home);
    success_json(cli.run(&["project", "pause", "pwf"]));
    let payload = add_payload("alt", "other", "/work/other", &absolute_tasks_path);

    let output = cli.run_with_home(
        &["project", "add", "--kind", "directory", &payload],
        Some(&home),
    );

    assert_failure(
        output,
        &[
            "managed projects ALT and PWF resolve to the same task location",
            &absolute_tasks_path,
        ],
    );
    assert_eq!(existing["tasks"]["path"], "~/tasks/shared");
    let stored = success_json(cli.run(&["project", "get", "pwf"]));
    assert_eq!(stored["tasks"]["path"], "~/tasks/shared");
}

#[test]
fn resume_rejects_a_runtime_task_path_owned_by_another_paused_project() {
    let cli = ProjectCli::new();
    let directory = tempfile::tempdir().unwrap();
    let seed_home = directory.path().join("seed-home");
    let runtime_home = directory.path().join("runtime-home");
    let absolute_tasks_path = runtime_home
        .join("tasks/shared")
        .to_string_lossy()
        .into_owned();
    cli.add_with_home("pwf", "pwf", "/work/pwf", "~/tasks/shared", &seed_home);
    cli.add_with_home(
        "alt",
        "other",
        "/work/other",
        &absolute_tasks_path,
        &seed_home,
    );
    success_json(cli.run(&["project", "pause", "pwf"]));
    success_json(cli.run(&["project", "pause", "alt"]));

    let output = cli.run_with_home(&["project", "resume", "pwf"], Some(&runtime_home));

    assert_failure(
        output,
        &[
            "managed projects ALT and PWF resolve to the same task location",
            &absolute_tasks_path,
        ],
    );
    let stored = success_json(cli.run(&["project", "get", "pwf"]));
    assert_eq!(stored["is_paused"], true);
}

#[test]
fn engine_composition_rejects_task_path_aliases_before_record_scans() {
    let cli = ProjectCli::new();
    let directory = tempfile::tempdir().unwrap();
    let seed_home = directory.path().join("seed-home");
    let runtime_home = directory.path().join("runtime-home");
    let absolute_tasks_path = runtime_home
        .join("missing/tasks/shared")
        .to_string_lossy()
        .into_owned();
    cli.add_with_home(
        "pwf",
        "pwf",
        "/work/pwf",
        "~/missing/tasks/shared",
        &seed_home,
    );
    cli.add_with_home(
        "alt",
        "other",
        "/work/other",
        &absolute_tasks_path,
        &seed_home,
    );

    let output = cli.run_with_home(&["list", "--project", "pwf"], Some(&runtime_home));

    assert_failure(
        output,
        &[
            "managed projects ALT and PWF resolve to the same task location",
            &absolute_tasks_path,
        ],
    );
}

fn write_rename_fixture(tasks_path: &Path) {
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
fn rename_migrates_project_registry_and_task_notes_as_one_cli_lifecycle() {
    let cli = ProjectCli::new();
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let source = directory.path().join("self/ssh-agent-phone-app");
    let destination_source = directory.path().join("self/mimux");
    let tasks = directory.path().join("pwf-db/self/ssh-agent-phone-app");
    let destination_tasks = directory.path().join("pwf-db/self/mimux");
    fs::create_dir_all(&source).unwrap();
    write_rename_fixture(&tasks);
    let created = cli.add_with_home(
        "SSH",
        "ssh-agent-phone-app",
        source.to_str().unwrap(),
        tasks.to_str().unwrap(),
        &home,
    );

    let renamed = success_json(cli.run_with_home(
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
        Some(&home),
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
    assert_eq!(success_json(cli.run(&["project", "get", "MUX"])), renamed);
    assert_failure(
        cli.run(&["project", "get", "SSH"]),
        &["project not found: SSH"],
    );
    let shown = cli.run(&["show", "MUX-0079"]);
    let shown_stdout = String::from_utf8(shown.stdout).unwrap();
    let shown_stderr = String::from_utf8(shown.stderr).unwrap();
    assert!(
        shown.status.success(),
        "show failed\nstdout: {shown_stdout}\nstderr: {shown_stderr}"
    );
    assert!(shown_stdout.contains("status: active"));
    assert!(shown_stdout.contains("Task body remains intact."));
    assert!(destination_tasks.join("MUX-0079.md").is_file());
    assert!(!destination_tasks.join("SSH-0079.md").exists());
    assert!(!tasks.exists());
}

#[test]
fn rename_restores_registry_and_task_notes_when_filesystem_commit_fails() {
    let cli = ProjectCli::new();
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let source = directory.path().join("self/ssh-agent-phone-app");
    let destination_source = directory.path().join("self/mimux");
    let tasks = directory.path().join("pwf-db/self/ssh-agent-phone-app");
    let destination_tasks = directory.path().join("missing-parent/mimux");
    fs::create_dir_all(&source).unwrap();
    write_rename_fixture(&tasks);
    let created = cli.add_with_home(
        "SSH",
        "ssh-agent-phone-app",
        source.to_str().unwrap(),
        tasks.to_str().unwrap(),
        &home,
    );

    let output = cli.run_with_home(
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
        Some(&home),
    );

    assert_failure(
        output,
        &[
            "project rename filesystem commit failed",
            "registry rollback succeeded",
        ],
    );
    assert_eq!(success_json(cli.run(&["project", "get", "SSH"])), created);
    assert_failure(
        cli.run(&["project", "get", "MUX"]),
        &["project not found: MUX"],
    );
    assert!(tasks.join("SSH-0079.md").is_file());
    assert!(!destination_tasks.exists());
    let staging = tasks
        .parent()
        .unwrap()
        .join(".ssh-agent-phone-app.pwf-rename-staging");
    assert!(!staging.exists());

    fs::create_dir_all(destination_tasks.parent().unwrap()).unwrap();
    let retried = success_json(cli.run_with_home(
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
        Some(&home),
    ));

    assert_project(
        &retried,
        "MUX",
        "mimux",
        destination_source.to_str().unwrap(),
        destination_tasks.to_str().unwrap(),
        false,
    );
    assert!(!tasks.exists());
    assert!(destination_tasks.join("MUX-0079.md").is_file());
}
