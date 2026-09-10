use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, ensure};
use plist::{Dictionary, Value};
use tokio::time::{Duration, Instant, sleep};

use super::{State, absolute_environment, checked, environment, output, process};
use crate::command::SERVER_INSTALL_COMMAND;

pub(super) struct Registration {
    path: PathBuf,
    domain: String,
}

impl Registration {
    pub(super) async fn discover() -> anyhow::Result<Self> {
        let uid = checked(process::Command::new("id").arg("-u")).await?;
        let uid = uid.trim().parse::<u32>()?;
        Ok(Self {
            path: absolute_environment("HOME")?.join("Library/LaunchAgents/pwf-server.plist"),
            domain: format!("gui/{uid}"),
        })
    }

    fn label(&self) -> String {
        format!("{}/pwf-server", self.domain)
    }

    async fn process_id(&self) -> anyhow::Result<Option<u32>> {
        let response =
            output(process::Command::new("launchctl").args(["print", &self.label()])).await?;
        if matches!(response.status.code(), Some(3 | 113)) {
            return Ok(None);
        }
        ensure!(
            response.status.success(),
            "reading pwf-server launch agent: {}",
            String::from_utf8_lossy(&response.stderr).trim()
        );
        let text = String::from_utf8(response.stdout)?;
        text.lines()
            .find_map(|line| line.trim().strip_prefix("pid = "))
            .map(str::parse)
            .transpose()
            .map_err(Into::into)
    }

    pub(super) async fn status(&self) -> anyhow::Result<State> {
        if self.process_id().await?.is_some() {
            return Ok(State::Running);
        }
        let response =
            output(process::Command::new("launchctl").args(["print", &self.label()])).await?;
        if response.status.success()
            && String::from_utf8_lossy(&response.stdout)
                .lines()
                .any(|line| {
                    line.trim()
                        .strip_prefix("last exit code = ")
                        .is_some_and(|code| code != "0")
                })
        {
            return Ok(State::Failed);
        }
        Ok(if self.path.is_file() {
            State::Stopped
        } else {
            State::NotInstalled
        })
    }

    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    pub(super) async fn diagnostic_command(&self) -> anyhow::Result<process::Command> {
        let plist = Value::from_file(&self.path)?;
        let values = plist
            .as_dictionary()
            .context("invalid launch agent plist")?;
        let program = values
            .get("ProgramArguments")
            .and_then(Value::as_array)
            .and_then(|args| args.first())
            .and_then(Value::as_string)
            .context("launch agent has no executable")?;
        let mut command = process::Command::new(program);
        for name in super::ENVIRONMENT_NAMES {
            command.env_remove(name);
        }
        if let Some(environment) = values
            .get("EnvironmentVariables")
            .and_then(Value::as_dictionary)
        {
            for (name, value) in environment {
                if super::ENVIRONMENT_NAMES.contains(&name.as_str()) {
                    command.env(
                        name,
                        value
                            .as_string()
                            .context("invalid launch environment value")?,
                    );
                }
            }
        }
        Ok(command)
    }

    pub(super) async fn failure_detail(&self) -> anyhow::Result<String> {
        checked(process::Command::new("launchctl").args(["print", &self.label()])).await
    }

    pub(super) async fn stop(&self) -> anyhow::Result<()> {
        let pid = self.process_id().await?;
        let response =
            output(process::Command::new("launchctl").args(["bootout", &self.label()])).await?;
        ensure!(
            response.status.success() || matches!(response.status.code(), Some(3 | 113)),
            "stopping pwf-server launch agent: {}",
            String::from_utf8_lossy(&response.stderr).trim()
        );
        if let Some(pid) = pid {
            let deadline = Instant::now() + Duration::from_secs(12);
            while output(process::Command::new("/bin/kill").args(["-0", &pid.to_string()]))
                .await?
                .status
                .success()
            {
                ensure!(
                    Instant::now() < deadline,
                    "pwf-server did not stop within 12 seconds"
                );
                sleep(Duration::from_millis(100)).await;
            }
        }
        Ok(())
    }

    // Other platforms register through a subprocess.
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    pub(super) async fn install(&self, server: &Path) -> anyhow::Result<()> {
        let mut values = Dictionary::new();
        values.insert("Label".into(), Value::String("pwf-server".into()));
        values.insert(
            "ProgramArguments".into(),
            Value::Array(vec![
                Value::String(server.to_str().context("server path is not UTF-8")?.into()),
                Value::String("--managed".into()),
            ]),
        );
        values.insert("RunAtLoad".into(), Value::Boolean(true));
        let mut keep_alive = Dictionary::new();
        keep_alive.insert("SuccessfulExit".into(), Value::Boolean(false));
        values.insert("KeepAlive".into(), Value::Dictionary(keep_alive));
        values.insert("ExitTimeOut".into(), Value::Integer(12.into()));
        values.insert("ThrottleInterval".into(), Value::Integer(2.into()));
        values.insert("Umask".into(), Value::Integer(0o077.into()));
        values.insert(
            "EnvironmentVariables".into(),
            Value::Dictionary(
                environment()
                    .into_iter()
                    .map(|(name, value)| (name, Value::String(value)))
                    .collect(),
            ),
        );
        fs::create_dir_all(
            self.path
                .parent()
                .context("launch agent path has no parent")?,
        )?;
        Value::Dictionary(values).to_file_xml(&self.path)?;
        Ok(())
    }

    pub(super) async fn start(&self) -> anyhow::Result<()> {
        ensure!(
            self.path.is_file(),
            "pwf-server is not installed - run {SERVER_INSTALL_COMMAND}"
        );
        let loaded = output(process::Command::new("launchctl").args(["print", &self.label()]))
            .await?
            .status
            .success();
        if loaded {
            checked(process::Command::new("launchctl").args(["kickstart", &self.label()])).await?;
        } else {
            checked(process::Command::new("launchctl").args(["enable", &self.label()])).await?;
            checked(
                process::Command::new("launchctl")
                    .arg("bootstrap")
                    .arg(&self.domain)
                    .arg(&self.path),
            )
            .await?;
        }
        Ok(())
    }

    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    pub(super) async fn uninstall(&self) -> anyhow::Result<()> {
        if self.path.exists() {
            fs::remove_file(&self.path)?;
        }
        Ok(())
    }
}
