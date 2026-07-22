use std::{fs, path::Path};

fn repo_path(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(path)
}

#[test]
fn root_justfile_is_a_typed_command_catalog() {
    let root = fs::read_to_string(repo_path("justfile")).expect("read root justfile");
    let shell = r#"["bash", "-eu", "-o", "pipefail", "-c"]"#;
    assert!(root.contains(&format!("set shell := {shell}")));
    assert!(root.contains(&format!("set windows-shell := {shell}")));
    assert!(root.contains("mod project 'just/project.just'"));
    assert!(root.contains("mod agents 'just/agents.just'"));
    assert!(!root.contains("mod md"));

    for dependency in [
        "build: project::build",
        "ship: project::ship",
        "fmt: project::fmt",
        "fmt-check: project::fmt-check",
        "lint: project::lint",
        "check: project::check",
        "test *args: (project::test args)",
        "install: project::install",
        "update *args: (project::update args)",
        "bootstrap *args: (agents::bootstrap args)",
    ] {
        assert!(
            root.contains(dependency),
            "root recipe should use typed dependency `{dependency}`"
        );
    }
}

#[test]
fn project_module_forwards_logic_to_xtask() {
    let project = fs::read_to_string(repo_path("just/project.just")).expect("read project module");
    let shell = r#"["bash", "-eu", "-o", "pipefail", "-c"]"#;
    assert!(project.contains(&format!("set shell := {shell}")));
    assert!(project.contains(&format!("set windows-shell := {shell}")));
    assert!(project.contains("set working-directory := '..'"));

    for verb in [
        "fmt",
        "fmt-check",
        "lint",
        "check",
        "fix",
        "test",
        "ship",
        "install",
        "update",
    ] {
        let forwarded =
            project.contains(&format!("-- {verb}\n")) || project.contains(&format!("-- {verb} "));
        assert!(
            forwarded,
            "`{verb}` recipe should forward into the xtask crate"
        );
    }
    assert!(!project.contains("shellspec"));
}

#[test]
fn retired_shellspec_suite_stays_removed() {
    assert!(!repo_path(".shellspec").exists());
    assert!(!repo_path("spec").exists());
    assert!(!repo_path("just/md.justfile").exists());
}
