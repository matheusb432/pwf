use std::fs;
use std::path::Path;

fn repo_path(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

#[test]
fn justfile_is_split_into_bash_domain_modules() {
    let root = fs::read_to_string(repo_path("justfile")).expect("read root justfile");

    assert!(
        root.contains(r#"set shell := ["bash", "-eu", "-o", "pipefail", "-c"]"#),
        "root justfile should use the bash recipe shell"
    );
    assert!(
        root.contains("mod pwf 'just/pwf.justfile'"),
        "root justfile should include the pwf domain module"
    );
    assert!(
        root.contains("mod handoffs 'just/handoffs.justfile'"),
        "root justfile should include the handoffs domain module"
    );
    assert!(
        root.contains("mod agents 'just/agents.justfile'"),
        "root justfile should include the agents domain module"
    );
    assert!(
        !root.contains("[group('quality')]"),
        "quality must not remain as a junk-drawer recipe group"
    );
    assert!(
        !root.contains(r#"set shell := ["pwsh""#),
        "PowerShell must not remain the default recipe shell"
    );
    assert!(
        !root.contains("test-conformance"),
        "retired conformance recipe should not remain in the root justfile"
    );

    for module in ["pwf", "handoffs", "agents"] {
        let path = format!("just/{module}.justfile");
        let body = fs::read_to_string(repo_path(&path)).unwrap_or_else(|err| {
            panic!("read {path}: {err}");
        });
        assert!(
            body.contains(r#"set shell := ["bash", "-eu", "-o", "pipefail", "-c"]"#),
            "{path} should use the bash recipe shell"
        );
        assert!(
            body.contains("set working-directory := '..'"),
            "{path} should run recipes from the repo root"
        );
        assert!(
            !body.contains("test-conformance"),
            "retired conformance recipe should not remain in {path}"
        );
        assert!(
            !body.contains("ShellSpec conformance"),
            "retired conformance wording should not remain in {path}"
        );
    }
}
