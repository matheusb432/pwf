//! CLI reporting for stable Cargo dependency policies.

use std::path::Path;

use anyhow::{Context, Result, bail};
use cargo_metadata::{DependencyKind, Metadata, MetadataCommand};

use crate::{
    paths,
    process::{self, Status},
    verb::Verb,
};

struct EdgePolicy {
    from: &'static str,
    label: &'static str,
    forbidden: &'static [&'static str],
    reason: &'static str,
}

const EDGE_POLICIES: [EdgePolicy; 5] = [
    EdgePolicy {
        from: "pwf_models",
        label: "models stay independent",
        forbidden: &[
            "gray_matter",
            "serde",
            "serde_json",
            "sqlx",
            "toml",
            "pwf_application",
            "pwf_infra",
            "pwf",
            "prompt-lanes",
            "xtask",
        ],
        reason: "models must not depend on wire formats, persistence, use cases, or process roots",
    },
    EdgePolicy {
        from: "pwf_application",
        label: "application stays independent of adapters",
        forbidden: &["pwf_infra", "pwf", "xtask"],
        reason: "infrastructure belongs behind application-owned boundaries",
    },
    EdgePolicy {
        from: "pwf_infra",
        label: "infra stays independent of process roots",
        forbidden: &["pwf", "xtask"],
        reason: "adapters must not depend on their runtime composition",
    },
    EdgePolicy {
        from: "prompt-lanes",
        label: "prompt lanes stays reusable",
        forbidden: &["pwf_models", "pwf_application", "pwf_infra", "pwf"],
        reason: "shared lane syntax must remain independent of PWF product crates",
    },
    EdgePolicy {
        from: "xtask",
        label: "xtask stays outside the product graph",
        forbidden: &[
            "pwf_models",
            "pwf_application",
            "pwf_infra",
            "pwf",
            "prompt-lanes",
        ],
        reason: "repository automation must not become a product dependency boundary",
    },
];

pub(crate) fn run(root: Option<&Path>) -> Result<()> {
    let repository_root = root.map_or_else(paths::repo_root, Path::to_path_buf);
    let manifest = repository_root.join("Cargo.toml");
    let metadata = MetadataCommand::new()
        .manifest_path(&manifest)
        .no_deps()
        .exec()
        .with_context(|| format!("read Cargo workspace metadata from {}", manifest.display()))?;
    let violations = collect_violations(&metadata);

    if violations.is_empty() {
        process::result(Verb::CHECK_ARCHITECTURE, Status::Pass);
        Ok(())
    } else {
        for violation in &violations {
            eprintln!("{violation}");
        }
        bail!("check-architecture found {} violation(s)", violations.len());
    }
}

fn collect_violations(metadata: &Metadata) -> Vec<String> {
    let mut violations = Vec::new();

    for policy in &EDGE_POLICIES {
        let Some(package) = metadata
            .workspace_packages()
            .into_iter()
            .find(|package| package.name == policy.from)
        else {
            continue;
        };
        for dependency in package
            .dependencies
            .iter()
            .filter(|dependency| dependency.kind == DependencyKind::Normal)
        {
            if policy.forbidden.contains(&dependency.name.as_str()) {
                let manifest = package
                    .manifest_path
                    .strip_prefix(&metadata.workspace_root)
                    .unwrap_or(&package.manifest_path);
                violations.push(format!(
                    "{manifest}: [{}] {} -> {}: {}",
                    policy.label, policy.from, dependency.name, policy.reason,
                ));
            }
        }
    }

    violations.sort();
    violations.dedup();
    violations
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use cargo_metadata::MetadataCommand;

    use super::collect_violations;

    #[test]
    fn rejects_forbidden_product_edges() {
        let workspace = tempfile::tempdir().expect("create temporary workspace");
        write_workspace(
            workspace.path(),
            &[
                (
                    "models",
                    "pwf_models",
                    "[dependencies]\nserde = { path = \"../serde\" }\n",
                ),
                (
                    "application",
                    "pwf_application",
                    "[dependencies]\npwf_infra = { path = \"../infra\" }\n",
                ),
                ("infra", "pwf_infra", ""),
                ("serde", "serde", ""),
            ],
        );

        assert_eq!(
            violations(workspace.path()),
            [
                "application/Cargo.toml: [application stays independent of adapters] \
                 pwf_application -> pwf_infra: infrastructure belongs behind application-owned \
                 boundaries",
                "models/Cargo.toml: [models stay independent] pwf_models -> serde: models must not \
                 depend on wire formats, persistence, use cases, or process roots",
            ]
        );
    }

    #[test]
    fn accepts_inward_and_development_edges() {
        let workspace = tempfile::tempdir().expect("create temporary workspace");
        write_workspace(
            workspace.path(),
            &[
                (
                    "models",
                    "pwf_models",
                    "[dev-dependencies]\npwf_infra = { path = \"../infra\" }\n",
                ),
                (
                    "application",
                    "pwf_application",
                    "[dependencies]\npwf_models = { path = \"../models\" }\n",
                ),
                ("infra", "pwf_infra", ""),
            ],
        );

        assert!(violations(workspace.path()).is_empty());
    }

    fn violations(root: &Path) -> Vec<String> {
        let metadata = MetadataCommand::new()
            .manifest_path(root.join("Cargo.toml"))
            .no_deps()
            .exec()
            .expect("read fixture metadata");
        collect_violations(&metadata)
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
}
