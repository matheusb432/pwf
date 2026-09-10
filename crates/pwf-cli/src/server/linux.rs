use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, ensure};

use super::{State, absolute_environment, checked, environment, output, process, wait_stopped};
use crate::command::{SERVER_DOCTOR_COMMAND, SERVER_INSTALL_COMMAND};

const UNIT: &str = "pwf-server.service";

pub(super) struct Registration {
    path: PathBuf,
}

impl Registration {
    // Discovery is asynchronous on macOS.
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    pub(super) async fn discover() -> anyhow::Result<Self> {
        let config = if std::env::var_os("XDG_CONFIG_HOME").is_some() {
            absolute_environment("XDG_CONFIG_HOME")?
        } else {
            absolute_environment("HOME")?.join(".config")
        };
        Ok(Self {
            path: config.join("systemd/user").join(UNIT),
        })
    }

    pub(super) async fn status(&self) -> anyhow::Result<State> {
        let response = output(systemctl("show").args([
            "--property=LoadState",
            "--property=ActiveState",
            "--property=MainPID",
            "--property=SubState",
            "--property=Result",
            "--property=ExecMainStatus",
        ]))
        .await?;
        let text = String::from_utf8(response.stdout)?;
        if text.lines().any(|line| line == "LoadState=not-found") {
            return Ok(State::NotInstalled);
        }
        ensure!(
            response.status.success(),
            "reading pwf-server service status: {}",
            String::from_utf8_lossy(&response.stderr).trim()
        );
        ensure!(
            text.lines().any(|line| line.starts_with("LoadState=")),
            "systemctl omitted LoadState"
        );
        if text
            .lines()
            .any(|line| matches!(line, "ActiveState=failed" | "SubState=auto-restart"))
        {
            return Ok(State::Failed);
        }
        if text.lines().any(|line| line == "ActiveState=activating") {
            return Ok(State::Starting);
        }
        let running = text.lines().any(|line| {
            matches!(line, "ActiveState=active" | "ActiveState=deactivating")
                || line.strip_prefix("MainPID=").is_some_and(|pid| pid != "0")
        });
        Ok(if running {
            State::Running
        } else {
            State::Stopped
        })
    }

    pub(super) async fn diagnostic_command(&self) -> anyhow::Result<process::Command> {
        let text = checked(systemctl("show").args([
            "--property=ExecStart",
            "--property=Environment",
            "--property=EnvironmentFiles",
            "--property=WorkingDirectory",
        ]))
        .await?;
        let property = |name| text.lines().find_map(|line| line.strip_prefix(name));
        ensure!(
            property("EnvironmentFiles=").is_none_or(str::is_empty),
            "Service uses EnvironmentFile; inspect that configuration with systemctl --user cat pwf-server before running `{SERVER_DOCTOR_COMMAND}` in its environment"
        );
        let program = property("ExecStart={ path=")
            .and_then(|value| value.split_once(" ; argv[]=").map(|(path, _)| path))
            .with_context(|| {
                format!("Cannot resolve the registered executable; run `{SERVER_INSTALL_COMMAND}`")
            })?;
        ensure!(
            !program.contains("\\x"),
            "Service executable uses unsupported systemd escaping; inspect systemctl --user cat pwf-server"
        );
        let mut command = process::Command::new(program);
        for name in super::ENVIRONMENT_NAMES {
            command.env_remove(name);
        }
        let manager_environment =
            checked(process::Command::new("systemctl").args(["--user", "show-environment"]))
                .await?;
        command.envs(
            manager_environment
                .lines()
                .filter_map(|line| line.split_once('='))
                .filter(|(name, _)| super::ENVIRONMENT_NAMES.contains(name)),
        );
        let environment = property("Environment=").unwrap_or_default();
        let entries = shell_words::split(environment)?;
        command.envs(
            entries
                .iter()
                .filter_map(|entry| entry.split_once('='))
                .filter(|(name, _)| super::ENVIRONMENT_NAMES.contains(name)),
        );
        if let Some(directory) = property("WorkingDirectory=") {
            let directory = directory.strip_prefix('!').unwrap_or(directory);
            if !directory.is_empty() {
                command.current_dir(directory);
            }
        }
        Ok(command)
    }

    pub(super) async fn failure_detail(&self) -> anyhow::Result<String> {
        let mut detail = checked(systemctl("show").args([
            "--property=ActiveState",
            "--property=SubState",
            "--property=Result",
            "--property=ExecMainStatus",
            "--property=NRestarts",
        ]))
        .await?;
        let logs = checked(process::Command::new("journalctl").args([
            "--user",
            "--unit",
            UNIT,
            "--lines=20",
            "--no-pager",
            "--output=cat",
        ]))
        .await;
        match logs {
            Ok(logs) => {
                detail.push_str("\nRecent service logs:\n");
                detail.push_str(logs.trim());
            }
            Err(error) => {
                write!(detail, "\nService logs unavailable: {error}")?;
            }
        }
        Ok(detail)
    }

    pub(super) async fn stop(&self) -> anyhow::Result<()> {
        if self.status().await? != State::NotInstalled {
            checked(&mut systemctl("stop")).await?;
            wait_stopped(self).await?;
        }
        Ok(())
    }

    pub(super) async fn install(&self, server: &Path) -> anyhow::Result<()> {
        let mut unit = format!(
            "[Unit]\nDescription=PWF local gRPC server\n\n[Service]\nType=simple\nExecStart={}\nRestart=on-failure\nRestartPreventExitStatus=78\nRestartSec=2s\nTimeoutStopSec=12s\nUMask=0077\n",
            quote(server.to_str().context("server path is not UTF-8")?)?.replace('$', "$$")
        );
        for (name, value) in environment() {
            writeln!(unit, "Environment={}", quote(&format!("{name}={value}"))?)?;
        }
        unit.push_str("\n[Install]\nWantedBy=default.target\n");
        fs::create_dir_all(self.path.parent().context("unit path has no parent")?)?;
        fs::write(&self.path, unit)?;
        checked(process::Command::new("systemctl").args(["--user", "daemon-reload"])).await?;
        checked(&mut systemctl("enable")).await?;
        Ok(())
    }

    pub(super) async fn start(&self) -> anyhow::Result<()> {
        ensure!(
            self.status().await? != State::NotInstalled,
            "pwf-server is not installed - run {SERVER_INSTALL_COMMAND}"
        );
        checked(&mut systemctl("start")).await?;
        Ok(())
    }

    pub(super) async fn uninstall(&self) -> anyhow::Result<()> {
        if self.status().await? == State::NotInstalled {
            return Ok(());
        }
        checked(&mut systemctl("disable")).await?;
        if self.path.exists() {
            fs::remove_file(&self.path)?;
        }
        checked(process::Command::new("systemctl").args(["--user", "daemon-reload"])).await?;
        Ok(())
    }
}

fn systemctl(action: &str) -> process::Command {
    let mut command = process::Command::new("systemctl");
    command.args(["--user", action, UNIT]);
    command
}

fn quote(value: &str) -> anyhow::Result<String> {
    ensure!(
        !value.contains(['\n', '\r', '\0']),
        "service paths and environment values cannot contain line breaks or NUL"
    );
    Ok(format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    ))
}
