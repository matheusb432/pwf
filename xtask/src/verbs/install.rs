use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use clap::Args;

use crate::{paths, process};

mod server_service;

#[derive(Args)]
pub(crate) struct UpdateArgs {
    /// Preview the installed binary change without touching the system.
    #[arg(long, visible_alias = "dry-run")]
    dry: bool,
    /// Skip the full check preflight (still builds + refreshes the binary).
    #[arg(short = 'f', long)]
    force: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Placed {
    Installed,
    Updated,
    Unchanged,
}

#[cfg(unix)]
fn place_binary(source: &Path, destination: &Path) -> Result<Placed> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let destination_metadata = destination.symlink_metadata();
    if destination_metadata
        .as_ref()
        .is_ok_and(|metadata| metadata.file_type().is_file())
        && fs::read(source)? == fs::read(destination)?
    {
        return Ok(Placed::Unchanged);
    }

    let temporary = destination.with_file_name(format!(".pwf.{}.xtask-tmp", std::process::id()));
    fs::copy(source, &temporary)?;
    if let Err(error) = fs::rename(&temporary, destination) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }

    Ok(if destination_metadata.is_ok() {
        Placed::Updated
    } else {
        Placed::Installed
    })
}

fn path_export_line(dir: &Path, path_var: &str, rc_contents: &str) -> Option<String> {
    let dir_str = dir.to_string_lossy();
    let on_path = env::split_paths(path_var).any(|e| e == dir);
    if on_path || rc_contents.contains(dir_str.as_ref()) {
        return None;
    }
    Some(format!("\nexport PATH=\"{dir_str}:$PATH\"\n"))
}

fn bin_name() -> &'static str {
    if cfg!(windows) { "pwf.exe" } else { "pwf" }
}

fn server_bin_name() -> &'static str {
    if cfg!(windows) {
        "pwf-server.exe"
    } else {
        "pwf-server"
    }
}

fn release_bin(name: &str) -> PathBuf {
    paths::repo_root().join("target").join("release").join(name)
}

pub(crate) fn install() -> Result<()> {
    #[cfg(windows)]
    {
        return process::run(
            "scoop install",
            Command::new("scoop").args(["install", "pwf.json"]),
        );
    }
    #[cfg(unix)]
    {
        place_unix(false)
    }
}

pub(crate) fn update(args: &UpdateArgs) -> Result<()> {
    if !args.force {
        process::run("quality check", Command::new("just").arg("check"))?;
    }
    #[cfg(windows)]
    {
        process::run(
            "cargo build",
            Command::new("cargo").args(["build", "--release"]),
        )?;
        let dest = dirs_scoop_pwf()?;
        if args.dry {
            eprintln!(
                "DRY-RUN: would copy {} -> {}",
                release_bin(bin_name()).display(),
                dest.display()
            );
        } else {
            std::fs::copy(release_bin(bin_name()), &dest)
                .with_context(|| format!("copying to {}", dest.display()))?;
            eprintln!("refreshed global pwf binary -> {}", dest.display());
        }
        return Ok(());
    }
    #[cfg(unix)]
    {
        place_unix(args.dry)
    }
}

#[cfg(unix)]
fn place_unix(dry: bool) -> Result<()> {
    let cli_source = release_bin(bin_name());
    let server_source = release_bin(server_bin_name());
    let directory = install_directory()?;
    let cli_destination = directory.join(bin_name());
    let server_destination = directory.join(server_bin_name());
    if dry {
        eprintln!(
            "DRY-RUN: would copy {} -> {}",
            cli_source.display(),
            cli_destination.display()
        );
        eprintln!(
            "DRY-RUN: would copy {} -> {} and refresh its user service",
            server_source.display(),
            server_destination.display()
        );
        return Ok(());
    }
    process::run(
        "cargo build",
        Command::new("cargo").args(["build", "--release", "-p", "pwf-cli", "-p", "pwf-server"]),
    )?;
    let service = server_service::prepare(&server_destination)?;
    let cli_placed = place_binary(&cli_source, &cli_destination)?;
    eprintln!("pwf binary {cli_placed:?} -> {}", cli_destination.display());
    if let Some(service) = service.as_ref()
        && service.unregister_if_installed()?
    {
        eprintln!("stopped and unregistered the existing pwf-server user service");
    }
    let server_placed = place_binary(&server_source, &server_destination)?;
    eprintln!(
        "pwf-server binary {server_placed:?} -> {}",
        server_destination.display()
    );
    if let Some(service) = service {
        service.install_and_start()?;
        eprintln!("pwf-server user service installed and started");
    }
    wire_path(&directory)?;
    Ok(())
}

#[cfg(unix)]
fn install_directory() -> Result<PathBuf> {
    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".local").join("bin"))
}

#[cfg(unix)]
fn wire_path(dir: &Path) -> Result<()> {
    let path_var = env::var("PATH").unwrap_or_default();
    let rc = PathBuf::from(env::var_os("HOME").context("HOME is not set")?).join(".zshrc");
    let rc_contents = fs::read_to_string(&rc).unwrap_or_default();
    if let Some(line) = path_export_line(dir, &path_var, &rc_contents) {
        use std::io::Write;
        let mut f = fs::OpenOptions::new().create(true).append(true).open(&rc)?;
        f.write_all(line.as_bytes())?;
        eprintln!(
            "added {} to PATH in {} (open a new shell)",
            dir.display(),
            rc.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn binary_install_replaces_a_target_symlink_with_a_copy() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("pwf-bin");
        fs::write(&source, b"v1").unwrap();
        let destination = dir.path().join("bin").join("pwf");
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&source, &destination).unwrap();

        assert_eq!(
            place_binary(&source, &destination).unwrap(),
            Placed::Updated
        );
        assert!(
            !destination
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(destination).unwrap(), b"v1");
    }

    #[cfg(unix)]
    #[test]
    fn binary_install_is_idempotent_and_updates_changed_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("pwf-bin");
        let destination = dir.path().join("bin").join("pwf");
        fs::write(&source, b"v1").unwrap();

        assert_eq!(
            place_binary(&source, &destination).unwrap(),
            Placed::Installed
        );
        assert_eq!(
            place_binary(&source, &destination).unwrap(),
            Placed::Unchanged
        );

        fs::write(&source, b"v2").unwrap();
        assert_eq!(
            place_binary(&source, &destination).unwrap(),
            Placed::Updated
        );
        assert_eq!(fs::read(destination).unwrap(), b"v2");
    }

    #[test]
    fn path_export_skips_when_already_present() {
        let dir = Path::new("/home/u/.local/bin");
        assert!(path_export_line(dir, "/usr/bin:/home/u/.local/bin", "").is_none());
        assert!(
            path_export_line(dir, "/usr/bin", "export PATH=\"/home/u/.local/bin:$PATH\"").is_none()
        );
    }

    #[test]
    fn path_export_emits_when_absent() {
        let dir = Path::new("/home/u/.local/bin");
        let line = path_export_line(dir, "/usr/bin", "").unwrap();
        assert!(line.contains("/home/u/.local/bin:$PATH"));
    }
}

#[cfg(windows)]
fn dirs_scoop_pwf() -> Result<PathBuf> {
    let home = env::var_os("USERPROFILE").context("USERPROFILE is not set")?;
    Ok(PathBuf::from(home)
        .join("scoop")
        .join("apps")
        .join("pwf")
        .join("current")
        .join("pwf.exe"))
}
