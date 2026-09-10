use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context as _, Result};
use clap::Args;

use crate::{paths, process};

#[derive(Args, Default)]
pub(crate) struct InstallArgs {
    /// Cargo installation root, overriding `CARGO_INSTALL_ROOT` and `install.root`.
    #[arg(long)]
    root: Option<PathBuf>,
}

#[derive(Args)]
pub(crate) struct UpdateArgs {
    #[command(flatten)]
    install: InstallArgs,
    /// Preview the ordered installation steps without changing the system.
    #[arg(long, visible_alias = "dry-run")]
    dry: bool,
    /// Skip the full check preflight.
    #[arg(short = 'f', long)]
    force: bool,
}

pub(crate) fn install(arguments: &InstallArgs) -> Result<()> {
    place(arguments, false)
}

pub(crate) fn update(arguments: &UpdateArgs) -> Result<()> {
    if !arguments.dry && !arguments.force {
        process::run("quality check", Command::new("just").arg("check"))?;
    }
    place(&arguments.install, arguments.dry)
}

fn place(arguments: &InstallArgs, dry: bool) -> Result<()> {
    let package = paths::repo_root().join("crates/pwf-app");
    let root = install_root(arguments.root.as_deref(), &package)?;
    let cli = root
        .join("bin")
        .join(format!("pwf{}", env::consts::EXE_SUFFIX));
    if dry {
        eprintln!("DRY-RUN: stage pwf and pwf-server in a temporary Cargo install root");
        eprintln!(
            "DRY-RUN: staged pwf server install --check (read-only database compatibility check)"
        );
        eprintln!("DRY-RUN: staged pwf server stop");
        eprintln!(
            "DRY-RUN: install both staged builds into {}",
            root.display()
        );
        eprintln!("DRY-RUN: {} server install", cli.display());
        return Ok(());
    }
    stage_and_install(&package, &root, Path::new("cargo"))
}

fn stage_and_install(package: &Path, root: &Path, cargo: &Path) -> Result<()> {
    let staging = tempfile::tempdir().context("creating binary staging directory")?;
    install_binaries(package, staging.path(), cargo)?;
    let name = format!("pwf{}", env::consts::EXE_SUFFIX);
    let staged_cli = staging.path().join("bin").join(&name);
    let cli = root.join("bin").join(&name);
    process::run(
        "check replacement server before stopping the service",
        Command::new(&staged_cli).args(["server", "install", "--check"]),
    )
    .context("update preflight failed; installed binaries and service were not changed")?;
    let backups = tempfile::tempdir().context("creating binary rollback directory")?;
    for binary in ["pwf", "pwf-server"] {
        let name = format!("{binary}{}", env::consts::EXE_SUFFIX);
        let installed = root.join("bin").join(&name);
        if installed.is_file() {
            fs::copy(&installed, backups.path().join(&name))?;
        }
    }
    process::run(
        "stop installed server",
        Command::new(&staged_cli).args(["server", "stop"]),
    )?;
    if let Err(error) = install_binaries(package, root, cargo) {
        restore_binaries(root, backups.path())?;
        return Err(error).context("binary installation failed; previous binaries restored. The server is stopped; run pwf server start after resolving the installation failure");
    }
    process::run(
        "register and start installed server",
        Command::new(&cli).args(["server", "install"]),
    )
    .context(
        "installed server did not become ready; run pwf doctor for the cause and recovery action",
    )
}

fn restore_binaries(root: &Path, backups: &Path) -> Result<()> {
    for binary in ["pwf", "pwf-server"] {
        let name = format!("{binary}{}", env::consts::EXE_SUFFIX);
        let backup = backups.join(&name);
        let installed = root.join("bin").join(&name);
        if backup.is_file() {
            fs::copy(&backup, &installed).with_context(|| {
                format!(
                    "restoring {} after failed installation",
                    installed.display()
                )
            })?;
        } else if installed.is_file() {
            fs::remove_file(&installed)?;
        }
    }
    Ok(())
}

fn install_binaries(package: &Path, root: &Path, cargo: &Path) -> Result<()> {
    process::run(
        "Cargo binary installation",
        Command::new(cargo)
            .args(["install", "--locked", "--force", "--path"])
            .arg(package)
            .args(["--root"])
            .arg(root)
            .args(["--bin", "pwf", "--bin", "pwf-server", "--target-dir"])
            .arg(paths::repo_root().join("target")),
    )
}

fn install_root(explicit: Option<&Path>, package: &Path) -> Result<PathBuf> {
    let cwd = env::current_dir()?;
    if let Some(root) = explicit {
        return Ok(cwd.join(root));
    }
    if let Some(root) = env::var_os("CARGO_INSTALL_ROOT") {
        return Ok(cwd.join(root));
    }
    let cargo_home = cargo_home(&cwd)?;
    for directory in package
        .ancestors()
        .map(|path| path.join(".cargo"))
        .chain(std::iter::once(cargo_home.clone()))
    {
        if let Some(root) = configured_root(&directory)? {
            return Ok(root);
        }
    }
    Ok(cargo_home)
}

fn cargo_home(cwd: &Path) -> Result<PathBuf> {
    if let Some(home) = env::var_os("CARGO_HOME") {
        return Ok(cwd.join(home));
    }
    #[cfg(windows)]
    let variable = "USERPROFILE";
    #[cfg(not(windows))]
    let variable = "HOME";
    Ok(
        PathBuf::from(env::var_os(variable).with_context(|| format!("{variable} is not set"))?)
            .join(".cargo"),
    )
}

fn configured_root(directory: &Path) -> Result<Option<PathBuf>> {
    let legacy = directory.join("config");
    let path = if legacy.is_file() {
        legacy
    } else {
        directory.join("config.toml")
    };
    if !path.is_file() {
        return Ok(None);
    }
    let config: toml::Value = toml::from_str(&fs::read_to_string(&path)?)
        .with_context(|| format!("reading {}", path.display()))?;
    let Some(root) = config.get("install").and_then(|value| value.get("root")) else {
        return Ok(None);
    };
    let root = root
        .as_str()
        .context("Cargo install.root must be a path string")?;
    Ok(Some(
        directory
            .parent()
            .context("Cargo config directory has no parent")?
            .join(root),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_config_root_resolves_from_its_config_parent() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join(".cargo");
        fs::create_dir(&config).unwrap();
        fs::write(config.join("config.toml"), "[install]\nroot = 'tools'\n").unwrap();
        assert_eq!(
            configured_root(&config).unwrap(),
            Some(root.path().join("tools"))
        );
    }
}
