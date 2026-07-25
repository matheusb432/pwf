//! Verifies architecture-rule behavior that depends on source paths.

use std::{fs, process::Command};

const GENERIC_FILENAME_RULE: &str =
    include_str!("../../rules/no-generic-application-filenames.yml");
const EXPORT_SURFACE_RULE: &str =
    include_str!("../../rules/application-operation-export-surface.yml");

#[test]
fn generic_application_filename_rule_preserves_descriptive_compound_names() {
    let directory = tempfile::tempdir().expect("temporary ast-grep project");
    let root = directory.path();
    let rules = root.join("rules");
    let feature = root.join("crates/application/src/pending_work");
    fs::create_dir_all(&rules).expect("rules directory");
    fs::create_dir_all(&feature).expect("application feature directory");
    fs::write(root.join("sgconfig.yml"), "ruleDirs:\n  - rules\n").expect("ast-grep configuration");
    fs::write(
        rules.join("no-generic-application-filenames.yml"),
        GENERIC_FILENAME_RULE,
    )
    .expect("generic filename rule");
    fs::write(feature.join("helper.rs"), "pub fn execute() {}\n")
        .expect("generic filename fixture");
    for file_name in [
        "commit_provenance.rs",
        "model_selection.rs",
        "project_registry.rs",
    ] {
        fs::write(feature.join(file_name), "pub fn execute() {}\n")
            .expect("descriptive filename fixture");
    }

    let output = Command::new("ast-grep")
        .args(["scan", "--report-style=short"])
        .current_dir(root)
        .output()
        .expect("run ast-grep");
    assert!(!output.status.success(), "generic filename must fail");
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(diagnostics.contains("helper.rs"), "{diagnostics}");
    for file_name in [
        "commit_provenance.rs",
        "model_selection.rs",
        "project_registry.rs",
    ] {
        assert!(!diagnostics.contains(file_name), "{diagnostics}");
    }
}

#[test]
fn application_export_rule_checks_nested_operations_and_preserves_exclusions() {
    let directory = tempfile::tempdir().expect("temporary ast-grep project");
    let root = directory.path();
    let rules = root.join("rules");
    let application = root.join("crates/application/src");
    let session = application.join("pending_work/session");
    fs::create_dir_all(&rules).expect("rules directory");
    fs::create_dir_all(session.join("ports")).expect("application ports directory");
    fs::create_dir_all(session.join("tests")).expect("application tests directory");
    fs::write(root.join("sgconfig.yml"), "ruleDirs:\n  - rules\n").expect("ast-grep configuration");
    fs::write(
        rules.join("application-operation-export-surface.yml"),
        EXPORT_SURFACE_RULE,
    )
    .expect("application export rule");
    fs::write(
        application.join("pending_work.rs"),
        "pub use pending_work::Request;\n",
    )
    .expect("feature facade fixture");
    fs::write(session.join("verify.rs"), "pub fn run() {}\n")
        .expect("nested invalid operation fixture");
    fs::write(
        session.join("reexport.rs"),
        "pub fn execute() {}\npub use std::mem::drop as run;\n",
    )
    .expect("nested invalid re-export fixture");
    fs::write(session.join("dispatch.rs"), "pub fn execute() {}\n")
        .expect("nested operation fixture");
    fs::write(
        session.join("dto.rs"),
        "pub fn execute() {}\npub use super::support::Response;\n",
    )
    .expect("nested DTO fixture");
    fs::write(
        session.join("model.rs"),
        "pub fn execute() {}\npub use super::support::State;\n",
    )
    .expect("nested model fixture");
    fs::write(
        session.join("ports.rs"),
        "pub fn execute() {}\npub use super::support::Client;\n",
    )
    .expect("nested port facade fixture");
    fs::write(
        session.join("tests.rs"),
        "pub fn execute() {}\npub use super::support::fixture;\n",
    )
    .expect("nested test fixture");
    fs::write(
        session.join("ports/client.rs"),
        "pub fn execute() {}\npub use super::support::Client;\n",
    )
    .expect("nested port fixture");
    fs::write(
        session.join("tests/fixture.rs"),
        "pub fn execute() {}\npub use super::support::fixture;\n",
    )
    .expect("nested test support fixture");
    fs::write(
        session.join("verify_tests.rs"),
        "pub fn execute() {}\npub use super::support::fixture;\n",
    )
    .expect("operation test support fixture");
    fs::write(
        application.join("pending_work/session.rs"),
        "pub use dispatch::{Request, Response};\n",
    )
    .expect("nested feature facade fixture");

    let output = Command::new("ast-grep")
        .args(["scan", "--report-style=short"])
        .current_dir(root)
        .output()
        .expect("run ast-grep");
    assert!(!output.status.success(), "nested public support must fail");
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(diagnostics.contains("session/verify.rs"), "{diagnostics}");
    assert!(diagnostics.contains("session/reexport.rs"), "{diagnostics}");
    for path in [
        "pending_work.rs",
        "pending_work/session.rs",
        "session/dispatch.rs",
        "session/dto.rs",
        "session/model.rs",
        "session/ports.rs",
        "session/tests.rs",
        "session/ports/client.rs",
        "session/tests/fixture.rs",
        "session/verify_tests.rs",
    ] {
        assert!(!diagnostics.contains(path), "{diagnostics}");
    }
}
