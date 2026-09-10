use std::path::Path;

use anyhow::{Context as _, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};

use super::{State, checked, environment, process};
use crate::command::SERVER_INSTALL_COMMAND;

const SCRIPT: &str = include_str!("windows.ps1");

pub(super) struct Registration;

impl Registration {
    // Discovery is asynchronous on macOS.
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    pub(super) async fn discover() -> anyhow::Result<Self> {
        Ok(Self)
    }

    async fn invoke(&self, action: &str, server: Option<&Path>) -> anyhow::Result<String> {
        let script = format!(
            "try {{\n{SCRIPT}\nexit 0\n}} catch {{ [Console]::Error.WriteLine($_.ToString()); exit 1 }}"
        );
        let encoded = STANDARD.encode(
            script
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        let mut command = process::Command::new("powershell.exe");
        command
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-EncodedCommand",
                &encoded,
            ])
            .env("PWF_SERVER_ACTION", action)
            .env(
                "PWF_SERVER_INSTALL_COMMAND",
                SERVER_INSTALL_COMMAND.to_string(),
            )
            .env(
                "PWF_SERVER_ENVIRONMENT",
                serde_json::to_string(&environment())?,
            );
        if let Some(server) = server {
            command.env("PWF_SERVER_PROGRAM", server);
        }
        checked(&mut command)
            .await
            .with_context(|| format!("pwf-server scheduled task {action} failed"))
    }

    pub(super) async fn status(&self) -> anyhow::Result<State> {
        match self.invoke("Status", None).await?.trim() {
            "running" => Ok(State::Running),
            "stopped" => Ok(State::Stopped),
            "not installed" => Ok(State::NotInstalled),
            "failed" => Ok(State::Failed),
            other => bail!("unknown pwf-server task state: {other}"),
        }
    }

    pub(super) async fn diagnostic_command(&self) -> anyhow::Result<process::Command> {
        let configuration: serde_json::Value =
            serde_json::from_str(&self.invoke("Configuration", None).await?)?;
        let program = configuration["program"].as_str().with_context(|| {
            format!("scheduled task has no diagnostic executable; run `{SERVER_INSTALL_COMMAND}`")
        })?;
        let mut command = process::Command::new(program);
        for name in super::ENVIRONMENT_NAMES {
            command.env_remove(name);
        }
        let environment: Vec<(String, String)> =
            serde_json::from_value(configuration["environment"].clone())?;
        command.envs(environment);
        Ok(command)
    }

    pub(super) async fn failure_detail(&self) -> anyhow::Result<String> {
        self.invoke("Failure", None).await
    }

    pub(super) async fn stop(&self) -> anyhow::Result<()> {
        self.invoke("Stop", None).await?;
        Ok(())
    }
    pub(super) async fn start(&self) -> anyhow::Result<()> {
        self.invoke("Start", None).await?;
        Ok(())
    }
    pub(super) async fn install(&self, server: &Path) -> anyhow::Result<()> {
        self.invoke("Install", Some(server)).await?;
        Ok(())
    }
    pub(super) async fn uninstall(&self) -> anyhow::Result<()> {
        self.invoke("Uninstall", None).await?;
        Ok(())
    }
}
