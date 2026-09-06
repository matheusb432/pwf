use std::collections::BTreeSet;

use anyhow::{Context as _, Result, ensure};
use cargo_metadata::{MetadataCommand, Package};

use crate::paths;

pub(crate) fn run() -> Result<()> {
    let metadata = MetadataCommand::new()
        .manifest_path(paths::repo_root().join("Cargo.toml"))
        .no_deps()
        .exec()?;
    let packages = metadata.workspace_packages();
    let application = packages
        .iter()
        .find(|package| package.name == "pwf-app")
        .context("workspace has no pwf-app package")?;
    let mut remaining = packages
        .iter()
        .copied()
        .filter(|package| {
            package
                .publish
                .as_ref()
                .is_none_or(|names| !names.is_empty())
        })
        .collect::<Vec<_>>();
    let names = remaining
        .iter()
        .map(|package| package.name.as_str())
        .collect::<BTreeSet<_>>();
    let requirement = format!("={}", application.version);
    for package in &remaining {
        ensure!(
            package.version == application.version,
            "{} must share pwf-app version {}",
            package.name,
            application.version
        );
        for dependency in &package.dependencies {
            if dependency.path.is_some() {
                ensure!(
                    names.contains(dependency.name.as_str()),
                    "{} depends on unpublished {}",
                    package.name,
                    dependency.name
                );
                ensure!(
                    dependency.req.to_string() == requirement,
                    "{} requires {} {}, expected {}",
                    package.name,
                    dependency.name,
                    dependency.req,
                    requirement
                );
            }
        }
    }
    let mut published = BTreeSet::new();
    let mut order = Vec::new();
    while !remaining.is_empty() {
        let index = remaining
            .iter()
            .position(|package| ready(package, &names, &published))
            .context("published workspace dependencies contain a cycle")?;
        let package = remaining.remove(index);
        published.insert(package.name.as_str());
        order.push(package.name.as_str());
    }
    for name in order {
        println!("cargo publish --locked -p {name}");
    }
    Ok(())
}

fn ready(package: &Package, names: &BTreeSet<&str>, published: &BTreeSet<&str>) -> bool {
    package.dependencies.iter().all(|dependency| {
        !names.contains(dependency.name.as_str()) || published.contains(dependency.name.as_str())
    })
}
