use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, ensure};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use service_manager::ServiceLevel;
use service_manager::{
    RestartPolicy, ServiceInstallCtx, ServiceLabel, ServiceManager, ServiceStartCtx, ServiceStatus,
    ServiceStatusCtx, ServiceStopCtx, ServiceUninstallCtx,
};

const SERVICE_LABEL: &str = "pwf-server";

pub(super) struct PreparedService {
    manager: Box<dyn ServiceManager>,
    label: ServiceLabel,
    install_context: ServiceInstallCtx,
}

impl PreparedService {
    pub(super) fn unregister_if_installed(&self) -> Result<bool> {
        unregister_if_installed(self.manager.as_ref(), &self.label)
    }

    pub(super) fn install_and_start(self) -> Result<()> {
        self.manager
            .install(self.install_context)
            .context("registering the pwf-server user service")?;
        self.manager
            .start(ServiceStartCtx { label: self.label })
            .context("starting the pwf-server user service")
    }
}

pub(super) fn prepare(program: &Path) -> Result<Option<PreparedService>> {
    let Some(manager) = user_service_manager()? else {
        return Ok(None);
    };
    let install_context = service_install_context(program.to_path_buf())?;
    let label = install_context.label.clone();
    Ok(Some(PreparedService {
        manager,
        label,
        install_context,
    }))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn user_service_manager() -> Result<Option<Box<dyn ServiceManager>>> {
    let mut manager = <dyn ServiceManager>::native()
        .context("detecting the platform service manager for pwf-server")?;
    manager
        .set_level(ServiceLevel::User)
        .context("selecting user-level service management for pwf-server")?;
    ensure!(
        manager
            .available()
            .context("checking the platform service manager for pwf-server")?,
        "the platform service manager for pwf-server is unavailable"
    );
    Ok(Some(manager))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn user_service_manager() -> Result<Option<Box<dyn ServiceManager>>> {
    Ok(None)
}

fn service_label() -> Result<ServiceLabel> {
    SERVICE_LABEL
        .parse()
        .context("parsing the static pwf-server service label")
}

fn unregister_if_installed(manager: &dyn ServiceManager, label: &ServiceLabel) -> Result<bool> {
    let status = manager
        .status(ServiceStatusCtx {
            label: label.clone(),
        })
        .context("reading the existing pwf-server user service status")?;
    if status == ServiceStatus::NotInstalled {
        return Ok(false);
    }
    if status == ServiceStatus::Running {
        manager
            .stop(ServiceStopCtx {
                label: label.clone(),
            })
            .context("stopping the existing pwf-server user service")?;
    }
    manager
        .uninstall(ServiceUninstallCtx {
            label: label.clone(),
        })
        .context("unregistering the existing pwf-server user service")?;
    Ok(true)
}

fn service_install_context(program: PathBuf) -> Result<ServiceInstallCtx> {
    ensure!(
        program.is_absolute(),
        "pwf-server service executable path must be absolute: {}",
        program.display()
    );
    #[cfg(target_os = "linux")]
    let contents = Some(service_unit(&program));
    #[cfg(not(target_os = "linux"))]
    let contents = None;
    Ok(ServiceInstallCtx {
        label: service_label()?,
        contents,
        program,
        args: Vec::new(),
        username: None,
        working_directory: None,
        environment: None,
        autostart: true,
        restart_policy: RestartPolicy::OnFailure {
            delay_secs: Some(2),
            max_retries: None,
            reset_after_secs: None,
        },
    })
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
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
    fn install_context_registers_a_restartable_login_service() {
        let server = PathBuf::from("/home/dev/.local/bin/pwf-server");
        let context = service_install_context(server.clone()).unwrap();

        assert_eq!(context.label.to_qualified_name(), SERVICE_LABEL);
        assert_eq!(context.program, server);
        assert!(context.args.is_empty());
        assert!(context.autostart);
        assert_eq!(
            context.restart_policy,
            RestartPolicy::OnFailure {
                delay_secs: Some(2),
                max_retries: None,
                reset_after_secs: None,
            }
        );
    }

    #[test]
    fn install_context_rejects_a_relative_server_path() {
        let error = service_install_context(PathBuf::from("pwf-server")).unwrap_err();

        assert!(error.to_string().contains("must be absolute"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn service_unit_runs_one_restartable_private_process() {
        let unit = service_unit(Path::new("/home/dev/.local/bin/pwf-server"));

        assert!(unit.contains("ExecStart=\"/home/dev/.local/bin/pwf-server\""));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("TimeoutStopSec=12s"));
        assert!(unit.contains("UMask=0077"));
        assert!(unit.contains("WantedBy=default.target"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn service_unit_escapes_systemd_specifiers_and_quotes() {
        let unit = service_unit(Path::new("/home/100%/pwf \"tools\"/pwf-server"));

        assert!(unit.contains("/home/100%%/pwf \\\"tools\\\"/pwf-server"));
    }
}
