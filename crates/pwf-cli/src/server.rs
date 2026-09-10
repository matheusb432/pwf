#[cfg(unix)]
use std::path::PathBuf;
use std::{
    path::Path,
    process::{Output, Stdio},
    time::Duration,
};

use anyhow::{Context as _, bail, ensure};
use clap::{Args, Subcommand};
use pwf_client::PwfClient;
use tokio::{
    io::AsyncReadExt as _,
    process,
    time::{sleep, timeout},
};

use crate::{
    command::{
        DOCTOR_COMMAND, SERVER_INSTALL_COMMAND, SERVER_RESTART_COMMAND, SERVER_START_COMMAND,
        SERVER_STATUS_COMMAND,
    },
    console::Console,
};

mod diagnostics;
pub(crate) use diagnostics::diagnose;
use diagnostics::preflight;

#[cfg(target_os = "linux")]
#[path = "server/linux.rs"]
mod native;
#[cfg(target_os = "macos")]
#[path = "server/macos.rs"]
mod native;
#[cfg(windows)]
#[path = "server/windows.rs"]
mod native;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
const READINESS_TIMEOUT: Duration = Duration::from_secs(15);
const OUTPUT_LIMIT: u64 = 64 * 1024;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Register login startup, start the sibling server and wait until ready.
    #[command(name = SERVER_INSTALL_COMMAND.name())]
    Install {
        /// Check the sibling server and database without changing registration or stopping the
        /// service.
        #[arg(long)]
        check: bool,
    },
    /// Stop and remove startup registration, retaining binaries, database and notes.
    Uninstall,
    /// Start the registered server and wait until ready.
    #[command(name = SERVER_START_COMMAND.name())]
    Start,
    /// Stop the registered server and wait until it exits.
    Stop,
    /// Stop, then start the registered server and wait until ready.
    #[command(name = SERVER_RESTART_COMMAND.name())]
    Restart,
    /// Show startup registration, health and client/server versions.
    #[command(name = SERVER_STATUS_COMMAND.name())]
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    NotInstalled,
    Stopped,
    Running,
    Failed,
    #[cfg(target_os = "linux")]
    Starting,
}

pub async fn run(arguments: Arguments, console: Console) -> anyhow::Result<String> {
    let registration = native::Registration::discover().await?;
    match arguments.command {
        Command::Install { check } => {
            let cli = std::env::current_exe()?.canonicalize()?;
            let server = cli.with_file_name(format!("pwf-server{}", std::env::consts::EXE_SUFFIX));
            ensure!(
                server.is_file(),
                "missing sibling {} - install pwf-app with both binaries first",
                server.display()
            );
            preflight(&mut process::Command::new(&server), console.error_color()).await?;
            if check {
                return Ok("pwf-server installation preflight passed".into());
            }
            registration.stop().await?;
            registration.install(&server).await?;
            registration.start().await?;
            wait_ready(&registration, console).await?;
            report_shadowing(&cli);
            Ok(format!(
                "pwf-server {} installed and ready\nexecutable: {}",
                env!("CARGO_PKG_VERSION"),
                server.display()
            ))
        }
        Command::Uninstall => {
            registration.stop().await?;
            registration.uninstall().await?;
            Ok("pwf-server uninstalled".into())
        }
        command @ (Command::Start | Command::Restart) => {
            preflight(
                &mut registration.diagnostic_command().await?,
                console.error_color(),
            )
            .await?;
            if matches!(command, Command::Restart) {
                registration.stop().await?;
            }
            registration.start().await?;
            wait_ready(&registration, console).await?;
            Ok("pwf-server ready".into())
        }
        Command::Stop => {
            registration.stop().await?;
            Ok("pwf-server stopped".into())
        }
        Command::Status => {
            let state = registration.status().await?;
            let health = timeout(Duration::from_secs(6), async {
                let client = PwfClient::connect_local().await?;
                Ok::<_, anyhow::Error>(client.health().await?)
            })
            .await;
            let (health, version) = match health {
                Ok(Ok(health)) => (
                    if health.serving {
                        "serving"
                    } else {
                        "not serving"
                    },
                    health.version.unwrap_or_else(|| "unknown".into()),
                ),
                _ => ("unavailable", "unknown".into()),
            };
            let state = match state {
                State::NotInstalled => "not installed",
                State::Stopped => "stopped",
                State::Running => "running",
                State::Failed => "failed",
                #[cfg(target_os = "linux")]
                State::Starting => "starting",
            };
            Ok(format!(
                "service: {state}\nhealth: {health}\nclient version: {}\nserver version: {version}{}",
                env!("CARGO_PKG_VERSION"),
                if health == "unavailable" {
                    format!("\nRun `{DOCTOR_COMMAND}` for the cause and recovery action.")
                } else {
                    String::new()
                }
            ))
        }
    }
}

async fn wait_ready(registration: &native::Registration, console: Console) -> anyhow::Result<()> {
    timeout(READINESS_TIMEOUT, async {
        loop {
            let state = registration.status().await?;
            if matches!(state, State::Failed | State::Stopped) {
                let mut command = registration.diagnostic_command().await?;
                preflight(&mut command, console.error_color()).await?;
            }
            if state == State::Failed {
                let detail = registration.failure_detail().await?;
                bail!("pwf-server failed during startup: {}\nRun `{DOCTOR_COMMAND}` for recovery actions.", detail.trim());
            }
            let Ok(client) = PwfClient::connect_local().await else {
                sleep(Duration::from_millis(100)).await;
                continue;
            };
            let Ok(health) = client.health().await else {
                sleep(Duration::from_millis(100)).await;
                continue;
            };
            ensure!(health.version.as_deref() == Some(env!("CARGO_PKG_VERSION")), "running server version {} differs from pwf {} - inspect {SERVER_STATUS_COMMAND} and reinstall the service", health.version.as_deref().unwrap_or("unknown"), env!("CARGO_PKG_VERSION"));
            if health.serving { return Ok(()); }
            sleep(Duration::from_millis(100)).await;
        }
    }).await.with_context(|| format!("pwf-server did not become ready within 15 seconds - run `{DOCTOR_COMMAND}` for the cause and recovery action"))?
}

#[cfg(target_os = "linux")]
async fn wait_stopped(registration: &native::Registration) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
    while matches!(
        registration.status().await?,
        State::Running | State::Starting
    ) {
        ensure!(
            tokio::time::Instant::now() < deadline,
            "pwf-server did not stop within 12 seconds"
        );
        sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}

async fn output(command: &mut process::Command) -> anyhow::Result<Output> {
    let program = command.as_std().get_program().to_owned();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("starting {}", program.to_string_lossy()))?;
    let stdout = child.stdout.take().context("missing subprocess stdout")?;
    let stderr = child.stderr.take().context("missing subprocess stderr")?;
    let mut stdout = stdout.take(OUTPUT_LIMIT + 1);
    let mut stderr = stderr.take(OUTPUT_LIMIT + 1);
    let mut out = Vec::new();
    let mut err = Vec::new();
    let result = timeout(COMMAND_TIMEOUT, async {
        let (status, _, _) = tokio::try_join!(
            child.wait(),
            stdout.read_to_end(&mut out),
            stderr.read_to_end(&mut err)
        )?;
        ensure!(
            out.len() as u64 <= OUTPUT_LIMIT && err.len() as u64 <= OUTPUT_LIMIT,
            "service command output exceeded 64 KiB"
        );
        Ok::<_, anyhow::Error>(Output {
            status,
            stdout: out,
            stderr: err,
        })
    })
    .await;
    if let Ok(result) = result {
        result
    } else {
        let _ = timeout(Duration::from_secs(2), child.kill()).await;
        bail!("{} timed out after 20 seconds", program.to_string_lossy())
    }
}

async fn checked(command: &mut process::Command) -> anyhow::Result<String> {
    let response = output(command).await?;
    ensure!(
        response.status.success(),
        "service command failed ({}): {}",
        response.status,
        String::from_utf8_lossy(&response.stderr).trim()
    );
    Ok(String::from_utf8(response.stdout)?)
}

#[cfg(unix)]
fn absolute_environment(name: &str) -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(std::env::var_os(name).with_context(|| format!("{name} is not set"))?);
    ensure!(path.is_absolute(), "{name} must be an absolute path");
    Ok(path)
}

const ENVIRONMENT_NAMES: [&str; 5] = [
    "PWF_DATABASE_PATH",
    "PWF_RUNTIME_DIR",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
];

fn environment() -> Vec<(String, String)> {
    ENVIRONMENT_NAMES
        .into_iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| (name.to_owned(), value))
        })
        .collect()
}

fn report_shadowing(cli: &Path) {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let name = cli.file_name().unwrap_or_default();
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if !candidate.is_file() {
            continue;
        }
        if candidate
            .canonicalize()
            .is_ok_and(|candidate| candidate != cli)
        {
            eprintln!(
                "{} precedes this installation on PATH - use {} or adjust PATH",
                candidate.display(),
                cli.display()
            );
        }
        return;
    }
    eprintln!(
        "add {} to PATH to use this installation",
        cli.parent().unwrap_or(cli).display()
    );
}
