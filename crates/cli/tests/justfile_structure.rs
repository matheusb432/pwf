use std::{fs, path::Path};

fn repo_path(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(path)
}

#[test]
fn justfile_forwards_logic_to_the_xtask_crate() {
    let root = fs::read_to_string(repo_path("justfile")).expect("read root justfile");
    assert!(
        root.contains(r#"set shell := ["bash", "-eu", "-o", "pipefail", "-c"]"#),
        "root justfile should use the bash recipe shell"
    );
    assert!(root.contains("mod pwf 'just/pwf.justfile'"));
    assert!(root.contains("mod agents 'just/agents.justfile'"));
    assert!(
        !root.contains("mod md"),
        "the md module is retired (xtask owns Markdown fmt)"
    );

    let pwf = fs::read_to_string(repo_path("just/pwf.justfile")).expect("read pwf module");
    assert!(
        pwf.contains("set working-directory := '..'"),
        "pwf module should run recipes from the repo root"
    );
    for verb in [
        "fmt",
        "fmt-check",
        "fix",
        "test",
        "smell-check-errors",
        "install",
        "update",
    ] {
        let forwarded =
            pwf.contains(&format!("-- {verb}\n")) || pwf.contains(&format!("-- {verb} "));
        assert!(
            forwarded,
            "`{verb}` recipe should forward into the xtask crate"
        );
    }
    assert!(!pwf.contains("shellspec"), "ShellSpec is retired");
    assert!(
        !pwf.contains("_require-shellspec"),
        "ShellSpec guard is retired"
    );
}

#[test]
fn shellspec_suite_is_removed() {
    assert!(
        !repo_path(".shellspec").exists(),
        ".shellspec should be deleted"
    );
    assert!(!repo_path("spec").exists(), "spec/ should be deleted");
    assert!(
        !repo_path("just/md.justfile").exists(),
        "just/md.justfile should be deleted"
    );
}
