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
        from: "pwf-domain",
        label: "domain stays independent",
        forbidden: &[
            "sqlx",
            "pwf-application",
            "pwf-infra",
            "pwf",
            "prompt-lanes",
            "xtask",
        ],
        reason: "domain values must not depend on persistence, use cases, or process roots",
    },
    EdgePolicy {
        from: "pwf-application",
        label: "application stays independent of adapters",
        forbidden: &["pwf-infra", "pwf", "xtask"],
        reason: "infrastructure belongs behind application-owned boundaries",
    },
    EdgePolicy {
        from: "pwf-infra",
        label: "infra stays independent of process roots",
        forbidden: &["pwf", "xtask"],
        reason: "adapters must not depend on their runtime composition",
    },
    EdgePolicy {
        from: "prompt-lanes",
        label: "prompt lanes stays reusable",
        forbidden: &["pwf-domain", "pwf-application", "pwf-infra", "pwf"],
        reason: "shared lane syntax must remain independent of PWF product crates",
    },
    EdgePolicy {
        from: "xtask",
        label: "xtask stays outside the product graph",
        forbidden: &[
            "pwf-domain",
            "pwf-application",
            "pwf-infra",
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
