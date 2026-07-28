//! Checks stable workspace dependency directions from Cargo metadata.

use std::path::Path;

use anyhow::{Context, Result};
use cargo_metadata::{Dependency, DependencyKind, MetadataCommand, Package};

/// Locates one architecture violation and explains the rejected dependency.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Violation {
    pub(crate) relative_path: String,
    pub(crate) line: usize,
    pub(crate) message: String,
}

/// Checks normal workspace dependencies.
pub(crate) fn run(repo_root: &Path) -> Result<Result<(), Vec<Violation>>> {
    let manifest = repo_root.join("Cargo.toml");
    let metadata = MetadataCommand::new()
        .manifest_path(&manifest)
        .no_deps()
        .exec()
        .with_context(|| {
            format!(
                "reading Cargo workspace metadata from {}",
                manifest.display()
            )
        })?;
    let workspace_root = metadata.workspace_root.as_std_path();
    let mut violations = Vec::new();

    for package in metadata.workspace_packages() {
        let Some(member) = member_directory(workspace_root, package) else {
            continue;
        };
        for dependency in package
            .dependencies
            .iter()
            .filter(|dependency| dependency.kind == DependencyKind::Normal)
        {
            check_dependency(
                workspace_root,
                package,
                &member,
                dependency,
                &mut violations,
            );
        }
    }

    violations.sort();
    violations.dedup();
    Ok(if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    })
}

fn check_dependency(
    workspace_root: &Path,
    package: &Package,
    member: &str,
    dependency: &Dependency,
    violations: &mut Vec<Violation>,
) {
    let target = workspace_target(workspace_root, dependency);
    let detail = match member {
        "crates/domain" if dependency.name == "sqlx" => {
            Some("crates/domain must remain independent of sqlx".to_string())
        }
        "crates/domain" => target.as_ref().map(|target| {
            format!("crates/domain must not depend on workspace member {target}")
        }),
        "crates/application" => target.as_ref().and_then(|target| {
            (target != "crates/domain" && !target.starts_with("shared/")).then(|| {
                format!(
                    "crates/application may depend only on crates/domain or shared/*, not {target}"
                )
            })
        }),
        "crates/infra" => target.as_ref().and_then(|target| {
            (target != "crates/application"
                && target != "crates/domain"
                && !target.starts_with("shared/"))
            .then(|| {
                format!(
                    "crates/infra may depend only on inward production crates or shared/*, not {target}"
                )
            })
        }),
        "xtask" => target
            .as_ref()
            .map(|target| format!("xtask must not depend on workspace member {target}")),
        _ if member.starts_with("shared/") => target.as_ref().and_then(|target| {
            target.starts_with("crates/").then(|| {
                format!("{member} is application-agnostic and must not depend on {target}")
            })
        }),
        _ => None,
    };

    if let Some(message) = detail {
        violations.push(Violation {
            relative_path: manifest_path(workspace_root, package),
            line: 1,
            message,
        });
    }
}

fn member_directory(workspace_root: &Path, package: &Package) -> Option<String> {
    let manifest = package.manifest_path.as_std_path();
    let relative = manifest.strip_prefix(workspace_root).ok()?;
    relative.parent().map(portable_path)
}

fn workspace_target(workspace_root: &Path, dependency: &Dependency) -> Option<String> {
    let path = dependency.path.as_ref()?.as_std_path();
    let relative = path.strip_prefix(workspace_root).ok()?;
    Some(portable_path(relative))
}

fn manifest_path(workspace_root: &Path, package: &Package) -> String {
    package
        .manifest_path
        .as_std_path()
        .strip_prefix(workspace_root)
        .map_or_else(|_| package.manifest_path.to_string(), portable_path)
}

fn portable_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
