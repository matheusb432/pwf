use std::{fs, path::Path};

use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn check_architecture_rejects_an_outward_application_edge() {
    let workspace = tempfile::tempdir().expect("create temporary workspace");
    write_workspace(
        workspace.path(),
        &[
            (
                "application",
                "pwf-application",
                "[dependencies]\npwf-infra = { path = \"../infra\" }\n",
            ),
            ("infra", "pwf-infra", ""),
        ],
    );

    xtask(workspace.path()).assert().failure().stderr(contains(
        "[application stays independent of adapters] pwf-application -> pwf-infra: \
             infrastructure belongs behind application-owned boundaries",
    ));
}

#[test]
fn check_architecture_accepts_inward_and_development_edges() {
    let workspace = tempfile::tempdir().expect("create temporary workspace");
    write_workspace(
        workspace.path(),
        &[
            (
                "domain",
                "pwf-domain",
                "[dev-dependencies]\npwf-infra = { path = \"../infra\" }\n",
            ),
            (
                "application",
                "pwf-application",
                "[dependencies]\npwf-domain = { path = \"../domain\" }\n",
            ),
            ("infra", "pwf-infra", ""),
        ],
    );

    xtask(workspace.path()).assert().success();
}

fn xtask(root: &Path) -> Command {
    let mut command = Command::cargo_bin("xtask").expect("compile xtask");
    command
        .env("CARGO_NET_OFFLINE", "true")
        .args(["check-architecture", root.to_str().expect("UTF-8 path")]);
    command
}

fn write_workspace(root: &Path, packages: &[(&str, &str, &str)]) {
    let members = packages
        .iter()
        .map(|(directory, _, _)| format!("\"{directory}\""))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        root.join("Cargo.toml"),
        format!("[workspace]\nresolver = \"3\"\nmembers = [{members}]\n"),
    )
    .expect("write workspace manifest");

    for (directory, name, dependencies) in packages {
        let package = root.join(directory);
        fs::create_dir_all(package.join("src")).expect("create package source directory");
        fs::write(
            package.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
                 {dependencies}"
            ),
        )
        .expect("write package manifest");
        fs::write(package.join("src/lib.rs"), "").expect("write package source");
    }
}
