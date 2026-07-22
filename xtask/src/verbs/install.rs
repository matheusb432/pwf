//! Installs or updates the release binary's global shim.
//!
//! Unix uses a symlink and shell PATH entry. Windows uses the manually certified Scoop path.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clap::Args;

use crate::{
    paths,
    process::{self, Status},
    verb::Verb,
    verbs::check,
};

/// Flags for the `update` verb.
#[derive(Args)]
pub(crate) struct UpdateArgs {
    /// Preview the shim change and run `--help` without touching the system.
    #[arg(long, visible_alias = "dry-run")]
    pub(crate) dry: bool,
    /// Skip the full check preflight (still builds + refreshes the shim).
    #[arg(short = 'f', long)]
    pub(crate) force: bool,
}

/// Describes a shim placement result.
#[derive(Debug, PartialEq, Eq)]
enum Linked {
    Created,
    Retargeted,
    Unchanged,
}

/// Ensures `link` points to `target` and reports whether it changed.
#[cfg(unix)]
fn ensure_symlink(target: &Path, link: &Path) -> Result<Linked> {
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent)?;
    }
    if link.read_link().is_ok_and(|current| current == target) {
        return Ok(Linked::Unchanged);
    }
    let existed = link.symlink_metadata().is_ok();
    if existed {
        fs::remove_file(link)?;
    }
    std::os::unix::fs::symlink(target, link)?;
    Ok(if existed {
        Linked::Retargeted
    } else {
        Linked::Created
    })
}

/// Returns a PATH export when `dir` is absent from both PATH and shell configuration.
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

fn release_bin() -> PathBuf {
    paths::repo_root()
        .join("target")
        .join("release")
        .join(bin_name())
}

/// Installs the global pwf shim.
pub(crate) fn install() -> Result<()> {
    #[cfg(windows)]
    {
        // The Windows Scoop path is manually certified.
        process::run("scoop install", "scoop", &["install", "pwf.json"])?;
        process::result(Verb::INSTALL, Status::Done);
        return Ok(());
    }
    #[cfg(unix)]
    {
        place_unix(false)?;
        process::result(Verb::INSTALL, Status::Done);
        Ok(())
    }
}

/// Rebuilds and refreshes the shim after the format preflight unless forced.
pub(crate) fn update(args: &UpdateArgs) -> Result<()> {
    if should_run_check_preflight(args.force) {
        check::run()?;
    }
    #[cfg(windows)]
    {
        // The Windows Scoop path is manually certified.
        process::run("cargo build", "cargo", &["build", "--release"])?;
        let dest = dirs_scoop_pwf()?;
        if args.dry {
            eprintln!(
                "DRY-RUN: would copy {} -> {}",
                release_bin().display(),
                dest.display()
            );
        } else {
            std::fs::copy(release_bin(), &dest)
                .with_context(|| format!("copying to {}", dest.display()))?;
            eprintln!("refreshed global pwf shim -> {}", dest.display());
        }
        process::result(Verb::UPDATE, Status::Done);
        return Ok(());
    }
    #[cfg(unix)]
    {
        place_unix(args.dry)?;
        process::result(Verb::UPDATE, Status::Done);
        Ok(())
    }
}

fn should_run_check_preflight(force: bool) -> bool {
    !force
}

/// Builds and links the Unix binary, then ensures its directory is on PATH.
#[cfg(unix)]
fn place_unix(dry: bool) -> Result<()> {
    let target = release_bin();
    let link = link_path()?;
    if dry {
        eprintln!(
            "DRY-RUN: would ensure symlink {} -> {}",
            link.display(),
            target.display()
        );
        return Ok(());
    }
    process::run("cargo build", "cargo", &["build", "--release"])?;
    let placed = ensure_symlink(&target, &link)?;
    eprintln!("pwf shim {placed:?} -> {}", target.display());
    wire_path(link.parent().context("link has no parent")?)?;
    Ok(())
}

/// Resolves `~/.local/bin/pwf`.
#[cfg(unix)]
fn link_path() -> Result<PathBuf> {
    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".local").join("bin").join("pwf"))
}

/// Appends a PATH export to `~/.bashrc` when needed.
#[cfg(unix)]
fn wire_path(dir: &Path) -> Result<()> {
    let path_var = env::var("PATH").unwrap_or_default();
    let rc = PathBuf::from(env::var_os("HOME").context("HOME is not set")?).join(".bashrc");
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
    fn symlink_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("pwf-bin");
        fs::write(&target, b"v1").unwrap();
        let link = dir.path().join("bin").join("pwf");
        assert_eq!(ensure_symlink(&target, &link).unwrap(), Linked::Created);
        assert_eq!(ensure_symlink(&target, &link).unwrap(), Linked::Unchanged);
        let other = dir.path().join("other");
        fs::write(&other, b"v2").unwrap();
        assert_eq!(ensure_symlink(&other, &link).unwrap(), Linked::Retargeted);
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
    fn update_check_preflight_is_skipped_only_when_forced() {
        assert!(should_run_check_preflight(false));
        assert!(!should_run_check_preflight(true));
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
