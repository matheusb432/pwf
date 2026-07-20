//! Conservative dependency-direction checks for workspace packages.

use std::{collections::HashSet, path::Path};

use anyhow::{Context, Result};
use cargo_metadata::{DependencyKind, MetadataCommand};

const DEPENDENCY_EDGES_FORBIDDEN: &[(&str, &str)] = &[
    ("pwf-domain", "pwf-application"),
    ("pwf-domain", "pwf-infra"),
    ("pwf-domain", "pwf"),
    ("pwf-application", "pwf-infra"),
    ("pwf-application", "pwf"),
    ("pwf-infra", "pwf"),
];

/// Locates one architecture violation and explains the rejected dependency.
pub(crate) struct Violation {
    pub(crate) relative_path: String,
    pub(crate) line: usize,
    pub(crate) message: String,
}

/// Checks normal workspace dependencies for forbidden outward edges.
pub(crate) fn run(repo_root: &Path) -> Result<Result<(), Vec<Violation>>> {
    let metadata = MetadataCommand::new()
        .current_dir(repo_root)
        .no_deps()
        .exec()
        .context("reading Cargo workspace metadata for architecture check")?;
    let package_names = metadata
        .workspace_packages()
        .into_iter()
        .map(|package| package.name.as_str())
        .collect::<HashSet<_>>();
    let mut dependency_edges_reported = HashSet::new();
    let mut violations = Vec::new();

    for package in metadata.workspace_packages() {
        for dependency in package.dependencies.iter().filter(|dependency| {
            dependency.kind == DependencyKind::Normal
                && package_names.contains(dependency.name.as_str())
                && DEPENDENCY_EDGES_FORBIDDEN
                    .contains(&(package.name.as_str(), dependency.name.as_str()))
        }) {
            if !dependency_edges_reported.insert((package.name.as_str(), dependency.name.as_str()))
            {
                continue;
            }
            let manifest_path = package
                .manifest_path
                .strip_prefix(&metadata.workspace_root)
                .map_or_else(|_| package.manifest_path.to_string(), ToString::to_string);
            violations.push(Violation {
                relative_path: manifest_path,
                line: 1,
                message: format!(
                    "{} must not depend on outward layer {}",
                    package.name, dependency.name
                ),
            });
        }
    }

    Ok(if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn workspace_with_dependencies(
        dependency_domain: &str,
        dependency_application: &str,
    ) -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("temporary workspace");
        fs::write(
            directory.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"3\"\n",
        )
        .expect("workspace manifest");

        for (directory_name, package_name, dependencies) in [
            ("domain", "pwf-domain", dependency_domain),
            ("application", "pwf-application", dependency_application),
            (
                "infra",
                "pwf-infra",
                "[dependencies]\npwf-application = { path = \"../application\" }\n",
            ),
            (
                "cli",
                "pwf",
                "[dependencies]\npwf-infra = { path = \"../infra\" }\n",
            ),
        ] {
            let package_directory = directory.path().join("crates").join(directory_name);
            fs::create_dir_all(package_directory.join("src")).expect("package source directory");
            fs::write(
                package_directory.join("Cargo.toml"),
                format!(
                    "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n{dependencies}"
                ),
            )
            .expect("package manifest");
            fs::write(package_directory.join("src/lib.rs"), "").expect("package source");
        }

        directory
    }

    #[test]
    fn dependency_policy_accepts_inward_edges() {
        let workspace = workspace_with_dependencies(
            "",
            "[dependencies]\npwf-domain = { path = \"../domain\" }\n",
        );

        assert!(run(workspace.path()).unwrap().is_ok());
    }

    #[test]
    fn dependency_policy_rejects_domain_to_application() {
        let workspace = workspace_with_dependencies(
            "[dependencies]\npwf-application = { path = \"../application\" }\n",
            "",
        );

        let violations = run(workspace.path()).unwrap().unwrap_err();

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].relative_path, "crates/domain/Cargo.toml");
        assert_eq!(violations[0].line, 1);
        assert_eq!(
            violations[0].message,
            "pwf-domain must not depend on outward layer pwf-application"
        );
    }

    #[test]
    fn dependency_policy_ignores_development_edges() {
        let workspace = workspace_with_dependencies(
            "[dev-dependencies]\npwf-application = { path = \"../application\" }\n",
            "",
        );

        assert!(run(workspace.path()).unwrap().is_ok());
    }
}
