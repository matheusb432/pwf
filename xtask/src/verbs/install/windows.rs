use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use anyhow::{Context as _, Result};

use crate::process;

const SERVICE_SCRIPT: &str = include_str!("windows_server.ps1");

pub(super) fn place(dry: bool) -> Result<()> {
    let directory = PathBuf::from(env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?)
        .join("Programs")
        .join("pwf");
    let cli = directory.join("pwf.exe");
    let server = directory.join("pwf-server.exe");
    if dry {
        eprintln!(
            "DRY-RUN: would refresh {} and {} and register server startup at sign-in",
            cli.display(),
            server.display()
        );
        return Ok(());
    }
    process::run(
        "cargo build",
        Command::new("cargo").args(["build", "--release", "-p", "pwf-cli", "-p", "pwf-server"]),
    )?;
    fs::create_dir_all(&directory)?;
    configure_server("Stop", &server)?;
    fs::copy(super::release_bin("pwf.exe"), &cli).context("refreshing the installed CLI")?;
    fs::copy(super::release_bin("pwf-server.exe"), &server)
        .context("refreshing the bundled server")?;
    fs::write(directory.join("pwf-server.ps1"), SERVICE_SCRIPT)?;
    configure_server("Install", &server)?;
    eprintln!("refreshed pwf and its server in {}", directory.display());
    Ok(())
}

fn configure_server(action: &str, program: &Path) -> Result<()> {
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", SERVICE_SCRIPT])
        .env("PWF_SERVER_ACTION", action)
        .env("PWF_SERVER_PROGRAM", program);
    process::run_bounded(
        "configure pwf-server sign-in startup",
        command,
        Duration::from_secs(30),
    )
}
