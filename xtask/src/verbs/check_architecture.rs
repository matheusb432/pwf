use std::path::Path;

use anyhow::{Context, Result, bail};
use cargo_metadata::{DependencyKind, Metadata, MetadataCommand};

use crate::paths;

struct EdgePolicy {
    from: &'static str,
    label: &'static str,
    forbidden: &'static [&'static str],
    reason: &'static str,
}

const EDGE_POLICIES: [EdgePolicy; 6] = [
    EdgePolicy {
        from: "pwf-models",
        label: "models stay independent",
        forbidden: &[
            "gray_matter",
            "serde",
            "serde_json",
            "sqlx",
            "toml",
            "pwf-wire",
            "pwf-application",
            "pwf-infra",
            "pwf-cli",
            "pwf-migrator",
            "prompt-lanes",
            "xtask",
        ],
        reason: "models must not depend on wire formats, persistence, use cases, or process roots",
    },
    EdgePolicy {
        from: "pwf-wire",
        label: "wire stays independent of use cases and adapters",
        forbidden: &[
            "pwf-application",
            "pwf-infra",
            "pwf-cli",
            "pwf-migrator",
            "xtask",
        ],
        reason: "wire contracts must not depend on use cases, adapters, or process roots",
    },
    EdgePolicy {
        from: "pwf-application",
        label: "application stays independent of adapters",
        forbidden: &["pwf-infra", "pwf-cli", "pwf-migrator", "xtask"],
        reason: "infrastructure belongs behind application-owned boundaries",
    },
    EdgePolicy {
        from: "pwf-infra",
        label: "infra stays independent of process roots",
        forbidden: &["pwf-cli", "pwf-migrator", "xtask"],
        reason: "adapters must not depend on their runtime composition",
    },
    EdgePolicy {
        from: "prompt-lanes",
        label: "prompt lanes stays reusable",
        forbidden: &[
            "pwf-models",
            "pwf-wire",
            "pwf-application",
            "pwf-infra",
            "pwf-cli",
            "pwf-migrator",
        ],
        reason: "shared lane syntax must remain independent of PWF product crates",
    },
    EdgePolicy {
        from: "xtask",
        label: "xtask stays outside the product graph",
        forbidden: &[
            "pwf-models",
            "pwf-wire",
            "pwf-application",
            "pwf-infra",
            "pwf-cli",
            "pwf-migrator",
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
        return Ok(());
    }

    for violation in &violations {
        eprintln!("{violation}");
    }
    bail!("check-architecture found {} violation(s)", violations.len());
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
                    "pwf-models",
                    "pwf-models",
                    "[dependencies]\nserde = { path = \"../serde\" }\n",
                ),
                (
                    "pwf-application",
                    "pwf-application",
                    "[dependencies]\npwf-infra = { path = \"../pwf-infra\" }\n",
                ),
                (
                    "pwf-wire",
                    "pwf-wire",
                    "[dependencies]\npwf-infra = { path = \"../pwf-infra\" }\n",
                ),
                (
                    "pwf-infra",
                    "pwf-infra",
                    "[dependencies]\npwf-migrator = { path = \"../pwf-migrator\" }\n",
                ),
                ("pwf-migrator", "pwf-migrator", ""),
                ("serde", "serde", ""),
            ],
        );

        assert_eq!(
            violations(workspace.path()),
            [
                "pwf-application/Cargo.toml: [application stays independent of adapters] \
                 pwf-application -> pwf-infra: infrastructure belongs behind application-owned \
                 boundaries",
                "pwf-infra/Cargo.toml: [infra stays independent of process roots] pwf-infra -> \
                 pwf-migrator: adapters must not depend on their runtime composition",
                "pwf-models/Cargo.toml: [models stay independent] pwf-models -> serde: models must not \
                 depend on wire formats, persistence, use cases, or process roots",
                "pwf-wire/Cargo.toml: [wire stays independent of use cases and adapters] pwf-wire -> \
                 pwf-infra: wire contracts must not depend on use cases, adapters, or process roots",
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
                    "pwf-models",
                    "pwf-models",
                    "[dev-dependencies]\npwf-infra = { path = \"../pwf-infra\" }\n",
                ),
                (
                    "pwf-application",
                    "pwf-application",
                    "[dependencies]\npwf-models = { path = \"../pwf-models\" }\npwf-wire = { path = \
                     \"../pwf-wire\" }\n",
                ),
                (
                    "pwf-wire",
                    "pwf-wire",
                    "[dependencies]\npwf-models = { path = \"../pwf-models\" }\nprompt-lanes = { path = \"../prompt-lanes\" }\n",
                ),
                (
                    "pwf-infra",
                    "pwf-infra",
                    "[dependencies]\npwf-wire = { path = \"../pwf-wire\" }\n",
                ),
                ("prompt-lanes", "prompt-lanes", ""),
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
