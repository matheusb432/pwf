use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context as _, Result};

use crate::process;

const SERVICE_NAME: &str = "pwf-server.service";

pub(super) fn install(server: &Path) -> Result<Option<PathBuf>> {
    let Some(config_home) = xdg_config_home()? else {
        return Ok(None);
    };
    let path = service_path(&config_home);
    write_service(&path, server)
        .with_context(|| format!("installing pwf-server user service at {}", path.display()))?;
    systemctl(
        "reload pwf-server user service",
        ["--user", "daemon-reload"],
    )?;
    systemctl(
        "enable pwf-server user service",
        ["--user", "enable", SERVICE_NAME],
    )?;
    systemctl(
        "restart pwf-server user service",
        ["--user", "restart", SERVICE_NAME],
    )?;
    Ok(Some(path))
}

fn systemctl<const N: usize>(label: &str, arguments: [&str; N]) -> Result<()> {
    process::run(label, Command::new("systemctl").args(arguments))
}

fn xdg_config_home() -> Result<Option<PathBuf>> {
    if env::consts::OS != "linux" {
        return Ok(None);
    }
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return Ok(Some(PathBuf::from(config_home)));
    }
    let home = env::var_os("HOME")
        .context("HOME is not set; cannot install the pwf-server user service")?;
    Ok(Some(PathBuf::from(home).join(".config")))
}

fn service_path(config_home: &Path) -> PathBuf {
    config_home.join("systemd").join("user").join(SERVICE_NAME)
}

fn write_service(path: &Path, server: &Path) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let contents = service_unit(server);
    if fs::read(path).is_ok_and(|existing| existing == contents.as_bytes()) {
        return Ok(());
    }
    let temporary = parent.join(format!(".{SERVICE_NAME}.{}.tmp", std::process::id()));
    fs::write(&temporary, contents)?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn service_unit(server: &Path) -> String {
    let executable = systemd_exec_argument(server);
    format!(
        "\
[Unit]\n\
Description=PWF local gRPC server\n\
\n\
[Service]\n\
Type=simple\n\
ExecStart={executable}\n\
Restart=on-failure\n\
RestartSec=2s\n\
TimeoutStopSec=12s\n\
UMask=0077\n\
\n\
[Install]\n\
WantedBy=default.target\n"
    )
}

fn systemd_exec_argument(path: &Path) -> String {
    let escaped = path
        .to_string_lossy()
        .replace('%', "%%")
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_unit_runs_one_restartable_private_process() {
        let unit = service_unit(Path::new("/home/dev/.local/bin/pwf-server"));

        assert!(unit.contains("ExecStart=\"/home/dev/.local/bin/pwf-server\""));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("TimeoutStopSec=12s"));
        assert!(unit.contains("UMask=0077"));
        assert!(unit.contains("WantedBy=default.target"));
    }

    #[test]
    fn service_unit_escapes_systemd_specifiers_and_quotes() {
        let unit = service_unit(Path::new("/home/100%/pwf \"tools\"/pwf-server"));

        assert!(unit.contains("/home/100%%/pwf \\\"tools\\\"/pwf-server"));
    }

    #[test]
    fn write_service_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let path = service_path(directory.path());
        let server = Path::new("/home/dev/.local/bin/pwf-server");

        write_service(&path, server).unwrap();
        let first = fs::read(&path).unwrap();
        write_service(&path, server).unwrap();

        assert_eq!(fs::read(path).unwrap(), first);
    }
}
