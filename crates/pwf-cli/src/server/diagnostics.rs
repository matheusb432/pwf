use anyhow::{Context as _, ensure};
use pwf_client::doctor::{Check, CheckStatus, DoctorReport};

use super::{State, native, output, process};
use crate::{
    command::{
        DOCTOR_COMMAND, SERVER_DOCTOR_COMMAND, SERVER_INSTALL_COMMAND, SERVER_START_COMMAND,
    },
    doctor::render_report,
};

pub(super) async fn preflight(
    command: &mut process::Command,
    color_on: bool,
) -> anyhow::Result<()> {
    let report = inspect(command).await?;
    ensure!(
        !report.failed(),
        "pwf-server cannot start:\n{}",
        render_report(&report, color_on)
    );
    Ok(())
}

async fn inspect(command: &mut process::Command) -> anyhow::Result<DoctorReport> {
    let program = command
        .as_std()
        .get_program()
        .to_string_lossy()
        .into_owned();
    let response = output(
        command
            .arg(SERVER_DOCTOR_COMMAND.name())
            .arg(format!("--{}", crate::doctor::JSON_ARGUMENT)),
    )
    .await?;
    let report: DoctorReport = serde_json::from_slice(&response.stdout).with_context(|| {
        format!(
            "{program} could not return a diagnostic report ({}): {}\nInstall matching binaries that support `{SERVER_DOCTOR_COMMAND}`.",
            response.status,
            String::from_utf8_lossy(&response.stderr).trim()
        )
    })?;
    ensure!(
        report.version == env!("CARGO_PKG_VERSION"),
        "{program} version {} differs from pwf {}; install matching binaries",
        report.version,
        env!("CARGO_PKG_VERSION")
    );
    ensure!(
        response.status.success() || report.failed(),
        "{program} exited with {} despite a passing report",
        response.status
    );
    Ok(report)
}

pub(crate) async fn diagnose() -> Vec<Check> {
    let mut checks = Vec::new();
    let cli = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            checks.push(Check::fail(
                "CLI executable",
                error.to_string(),
                "Check this installation's executable path.",
            ));
            return checks;
        }
    };
    checks.push(Check::pass(
        "CLI executable",
        format!("{} (version {})", cli.display(), env!("CARGO_PKG_VERSION")),
    ));
    let command = inspect_registration(&cli, &mut checks).await;
    match command {
        Ok(mut command) => {
            checks.push(Check::pass(
                "server executable",
                command.as_std().get_program().to_string_lossy(),
            ));
            match inspect(&mut command).await {
                Ok(report) => append_server_checks(&mut checks, report),
                Err(error) => checks.push(Check::fail(
                    "server diagnostics",
                    format!("{error:#}"),
                    "Check the registered executable and install a compatible release with doctor support.",
                )),
            }
        }
        Err(error) => checks.push(Check::fail(
            "service configuration",
            format!("{error:#}"),
            &format!("Inspect the native service registration and logs; run `{SERVER_INSTALL_COMMAND}` to register the intended sibling server."),
        )),
    }
    checks
}

async fn inspect_registration(
    cli: &std::path::Path,
    checks: &mut Vec<Check>,
) -> anyhow::Result<process::Command> {
    let registration = native::Registration::discover().await?;
    let state = registration.status().await?;
    checks.push(match state {
        State::NotInstalled => Check::fail(
            "service",
            "Not installed",
            &format!("Run `{SERVER_INSTALL_COMMAND}`."),
        ),
        State::Stopped => Check::fail(
            "service",
            "Stopped",
            &format!("Resolve failed checks, then run `{SERVER_START_COMMAND}`."),
        ),
        #[cfg(target_os = "linux")]
        State::Starting => Check::warning(
            "service",
            "Starting",
            &format!("Wait for startup, then run `{DOCTOR_COMMAND}` again."),
        ),
        State::Running => Check::pass("service", "Running"),
        State::Failed => Check::fail(
            "service",
            registration
                .failure_detail()
                .await
                .unwrap_or_else(|error| format!("{error:#}"))
                .trim(),
            &format!("Resolve the startup failure below, then run `{SERVER_START_COMMAND}`."),
        ),
    });
    if state == State::NotInstalled {
        Ok(process::Command::new(cli.with_file_name(format!(
            "pwf-server{}",
            std::env::consts::EXE_SUFFIX
        ))))
    } else {
        registration.diagnostic_command().await
    }
}

fn append_server_checks(checks: &mut Vec<Check>, report: DoctorReport) {
    if let Ok(endpoint) = pwf_client::local_endpoint_path()
        && report.checks.iter().any(|check| {
            check.name == "endpoint"
                && check.status == CheckStatus::Pass
                && check.detail != endpoint.display().to_string()
        })
    {
        checks.push(Check::fail(
            "endpoint configuration",
            "The CLI and registered service use different endpoints",
            &format!("Use the registered PWF_RUNTIME_DIR in this shell, or run `{SERVER_INSTALL_COMMAND}` with the intended configuration."),
        ));
    }
    checks.extend(report.checks);
}
