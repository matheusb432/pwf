//! `install` / `update` — build the pwf release binary and place it on PATH via a symlink.
//!
//! Migrates the bash `install`/`update` recipes. The testable core (idempotent symlink
//! placement, PATH-append decision) is split into pure helpers; the cargo-build and rc-file
//! glue is the thin I/O layer. The Windows scoop path is `cfg!(windows)`-gated and certified
//! manually (Linux is the primary host).

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clap::Args;

use crate::{
    paths,
    proc::{self, Status},
    verbs::fmt,
};

/// Flags for the `update` verb.
#[derive(Args)]
pub(crate) struct UpdateArgs {
    /// Preview the shim change and run `--help` without touching the system.
    #[arg(long, visible_alias = "dry-run")]
    pub(crate) dry: bool,
    /// Skip the fmt-check preflight (still builds + refreshes the shim).
    #[arg(short = 'f', long)]
    pub(crate) force: bool,
}

/// What ensuring the shim symlink did.
#[derive(Debug, PartialEq, Eq)]
enum Linked {
    Created,
    Retargeted,
    Unchanged,
}

/// Ensure `link` is a symlink pointing at `target`, idempotently. Filesystem-touching but pure.
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

/// Decide whether `dir` must be appended to a shell rc: `Some(export line)` when `dir` is neither
/// already on `$PATH` nor already written into `rc_contents`. Pure — the caller does the write.
fn path_export_line(dir: &Path, path_var: &str, rc_contents: &str) -> Option<String> {
    let dir_str = dir.to_string_lossy();
    let on_path = env::split_paths(path_var).any(|e| e == dir);
    if on_path || rc_contents.contains(dir_str.as_ref()) {
        return None;
    }
    Some(format!("\nexport PATH=\"{dir_str}:$PATH\"\n"))
}

/// Platform binary name for the built pwf artifact.
fn bin_name() -> &'static str {
    if cfg!(windows) { "pwf.exe" } else { "pwf" }
}

/// The built release binary path.
fn release_bin() -> PathBuf {
    paths::repo_root()
        .join("target")
        .join("release")
        .join(bin_name())
}

/// First-time setup of the global pwf shim.
pub(crate) fn install() -> Result<()> {
    #[cfg(windows)]
    {
        // Certified manually: scoop is the Windows install path.
        proc::run("scoop install", "scoop", &["install", "pwf.json"])?;
        proc::result("install", Status::Done);
        return Ok(());
    }
    #[cfg(unix)]
    {
        place_unix(false)?;
        proc::result("install", Status::Done);
        Ok(())
    }
}

/// Rebuild and refresh the installed shim; fmt-check preflight unless `--force`.
pub(crate) fn update(args: &UpdateArgs) -> Result<()> {
    if !args.force {
        fmt::fmt_check()?;
    }
    #[cfg(windows)]
    {
        // Certified manually: copy into the scoop shim dir.
        proc::run("cargo build", "cargo", &["build", "--release"])?;
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
        proc::result("update", Status::Done);
        return Ok(());
    }
    #[cfg(unix)]
    {
        place_unix(args.dry)?;
        proc::result("update", Status::Done);
        Ok(())
    }
}

/// Build (unless dry) and ensure `~/.local/bin/pwf` links the release binary + PATH is wired.
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
    proc::run("cargo build", "cargo", &["build", "--release"])?;
    let placed = ensure_symlink(&target, &link)?;
    eprintln!("pwf shim {placed:?} -> {}", target.display());
    wire_path(link.parent().context("link has no parent")?)?;
    Ok(())
}

/// `~/.local/bin/pwf`.
#[cfg(unix)]
fn link_path() -> Result<PathBuf> {
    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".local").join("bin").join("pwf"))
}

/// Append the PATH export to `~/.bashrc` when `dir` is not yet reachable.
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
