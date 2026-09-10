use anstyle::AnsiColor;
use clap::Args;
use pwf_client::{
    PwfClient,
    doctor::{Check, CheckStatus, DoctorReport},
};

use crate::{
    command::{SERVER_INSTALL_COMMAND, SERVER_RESTART_COMMAND, SERVER_START_COMMAND},
    console::Console,
    render::paint,
};

pub(crate) const JSON_ARGUMENT: &str = "json";

#[derive(Args, Debug)]
pub struct Arguments {
    /// Print a machine-readable report; failed checks return exit status 1.
    #[arg(long = JSON_ARGUMENT)]
    json: bool,
}

pub(crate) async fn inspect() -> DoctorReport {
    let mut checks = crate::server::diagnose().await;
    match pwf_client::local_endpoint_path() {
        Ok(path) => checks.push(Check::pass("client endpoint", path.display().to_string())),
        Err(error) => checks.push(Check::fail(
            "client endpoint",
            error.to_string(),
            "Correct PWF_RUNTIME_DIR and the runtime directory permissions.",
        )),
    }
    let health = tokio::time::timeout(std::time::Duration::from_secs(6), async {
        let client = PwfClient::connect_local().await?;
        Ok::<_, anyhow::Error>(client.health().await?)
    })
    .await;
    checks.push(match health {
        Ok(Ok(health))
            if health.serving && health.version.as_deref() == Some(env!("CARGO_PKG_VERSION")) =>
        {
            Check::pass(
                "RPC",
                format!("Serving, version {}", env!("CARGO_PKG_VERSION")),
            )
        }
        Ok(Ok(health)) => Check::fail(
            "RPC",
            format!(
                "serving: {}, server version: {}",
                health.serving,
                health.version.as_deref().unwrap_or("unknown")
            ),
            &format!(
                "Install matching pwf and pwf-server binaries, then run `{SERVER_INSTALL_COMMAND}`."
            ),
        ),
        Ok(Err(error)) => Check::fail(
            "RPC",
            format!("{error:#}"),
            &format!("Resolve the failed checks above, then run `{SERVER_START_COMMAND}`."),
        ),
        Err(_) => Check::fail(
            "RPC",
            "Health check timed out after 6 seconds",
            &format!("Inspect the native service logs and run `{SERVER_RESTART_COMMAND}`."),
        ),
    });
    DoctorReport {
        version: env!("CARGO_PKG_VERSION").into(),
        checks,
    }
}

impl Arguments {
    pub fn render(&self, report: &DoctorReport, console: Console) -> anyhow::Result<String> {
        if self.json {
            Ok(serde_json::to_string(report)?)
        } else {
            Ok(render_report(report, console.color()))
        }
    }
}

pub(crate) fn render_report(report: &DoctorReport, color_on: bool) -> String {
    report
        .checks
        .iter()
        .map(|check| {
            let (status, color) = match check.status {
                CheckStatus::Pass => ("OK", AnsiColor::Green),
                CheckStatus::Warning => ("WARN", AnsiColor::Yellow),
                CheckStatus::Fail => ("FAIL", AnsiColor::Red),
            };
            let mut output = format!("{} :: {}", paint(status, color, color_on), check.name);
            for line in check.detail.lines() {
                output.push_str("\n  ");
                output.push_str(line);
            }
            if let Some(action) = &check.action {
                output.push_str("\n  Action: ");
                output.push_str(&action.replace('\n', "\n    "));
            }
            output
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_details_and_recovery_stay_under_their_check() {
        let report = DoctorReport {
            version: "test".into(),
            checks: vec![Check::fail(
                "service",
                "failed\nrecent log",
                "Restart.\nRetry.",
            )],
        };
        assert_eq!(
            render_report(&report, false),
            "**FAIL** :: service\n  failed\n  recent log\n  Action: Restart.\n    Retry."
        );
    }
}
