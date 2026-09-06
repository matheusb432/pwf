use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, ensure};

use super::{State, absolute_environment, checked, environment, output, process, wait_stopped};

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
        let response = output(process::Command::new("systemctl").args([
            "--user",
            "show",
            UNIT,
            "--property=LoadState",
            "--property=ActiveState",
            "--property=MainPID",
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
        let running = text.lines().any(|line| {
            matches!(
                line,
                "ActiveState=active" | "ActiveState=activating" | "ActiveState=deactivating"
            ) || line.strip_prefix("MainPID=").is_some_and(|pid| pid != "0")
        });
        Ok(if running {
            State::Running
        } else {
            State::Stopped
        })
    }

    pub(super) async fn stop(&self) -> anyhow::Result<()> {
        if self.status().await? != State::NotInstalled {
            checked(process::Command::new("systemctl").args(["--user", "stop", UNIT])).await?;
            wait_stopped(self).await?;
        }
        Ok(())
    }

    pub(super) async fn install(&self, server: &Path) -> anyhow::Result<()> {
        let mut unit = format!(
            "[Unit]\nDescription=PWF local gRPC server\n\n[Service]\nType=simple\nExecStart={}\nRestart=on-failure\nRestartSec=2s\nTimeoutStopSec=12s\nUMask=0077\n",
            quote(server.to_str().context("server path is not UTF-8")?)?.replace('$', "$$")
        );
        for (name, value) in environment() {
            writeln!(unit, "Environment={}", quote(&format!("{name}={value}"))?)?;
        }
        unit.push_str("\n[Install]\nWantedBy=default.target\n");
        fs::create_dir_all(self.path.parent().context("unit path has no parent")?)?;
        fs::write(&self.path, unit)?;
        checked(process::Command::new("systemctl").args(["--user", "daemon-reload"])).await?;
        checked(process::Command::new("systemctl").args(["--user", "enable", UNIT])).await?;
        Ok(())
    }

    pub(super) async fn start(&self) -> anyhow::Result<()> {
        ensure!(
            self.status().await? != State::NotInstalled,
            "pwf-server is not installed - run pwf server install"
        );
        checked(process::Command::new("systemctl").args(["--user", "start", UNIT])).await?;
        Ok(())
    }

    pub(super) async fn uninstall(&self) -> anyhow::Result<()> {
        if self.status().await? == State::NotInstalled {
            return Ok(());
        }
        checked(process::Command::new("systemctl").args(["--user", "disable", UNIT])).await?;
        if self.path.exists() {
            fs::remove_file(&self.path)?;
        }
        checked(process::Command::new("systemctl").args(["--user", "daemon-reload"])).await?;
        Ok(())
    }
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
