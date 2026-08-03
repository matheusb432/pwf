use std::{
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use anyhow::{Context, Result, bail};

use crate::{child_process, sqlite_url};

const VERSION: &str = "0.9.0";
const CAPABILITY_DEADLINE: Duration = Duration::from_secs(15);
const INSTALL_DEADLINE: Duration = Duration::from_mins(10);
const VERSION_DEADLINE: Duration = Duration::from_secs(5);

pub(crate) fn ensure(root: &Path) -> Result<PathBuf> {
    let executable = sqlx_executable_path(root, cfg!(windows));
    let version = if executable.is_file() {
        version_probe(&executable)?
    } else {
        VersionProbe::Missing
    };
    let capabilities_available =
        version == VersionProbe::Matches && required_capabilities_available(root, &executable)?;
    let action = installation_action(version, capabilities_available);

    if action != InstallationAction::Keep {
        run(
            root,
            Path::new("cargo"),
            "sqlx-cli installation",
            &install_arguments(root, action),
            &[],
            INSTALL_DEADLINE,
        )?;
        if version_probe(&executable)? != VersionProbe::Matches
            || !required_capabilities_available(root, &executable)?
        {
            bail!(
                "repository SQLx CLI is not version {VERSION} with SQLite and sqlx.toml support after installation"
            );
        }
    }

    Ok(executable)
}

pub(crate) fn run(
    root: &Path,
    program: &Path,
    label: &str,
    arguments: &[OsString],
    environment: &[(&str, &str)],
    deadline: Duration,
) -> Result<()> {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .current_dir(root)
        .envs(environment.iter().copied());
    let status = child_process::run(command, label, deadline)?;
    if !status.success() {
        bail!("{label} failed (exit {})", status.code().unwrap_or(-1));
    }
    Ok(())
}

pub(crate) fn run_with_success_summary(
    root: &Path,
    program: &Path,
    label: &str,
    arguments: &[OsString],
    environment: &[(&str, &str)],
    deadline: Duration,
    success_summary: &str,
) -> Result<()> {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .current_dir(root)
        .envs(environment.iter().copied());
    run_command_with_success_summary(
        command,
        label,
        deadline,
        success_summary,
        &mut std::io::stderr().lock(),
    )
}

fn run_command_with_success_summary(
    mut command: Command,
    label: &str,
    deadline: Duration,
    success_summary: &str,
    diagnostics: &mut impl Write,
) -> Result<()> {
    let stdout = tempfile::NamedTempFile::new()
        .with_context(|| format!("creating captured stdout for {label}"))?;
    let stdout_child = stdout
        .reopen()
        .with_context(|| format!("opening captured stdout for {label}"))?;
    command.stdout(Stdio::from(stdout_child));

    let status = child_process::run(command, label, deadline);
    let stdout = std::fs::read(stdout.path())
        .with_context(|| format!("reading captured stdout for {label}"))?;

    match status {
        Ok(status) if status.success() => {
            writeln!(diagnostics, "{success_summary}")
                .with_context(|| format!("writing success summary for {label}"))?;
            Ok(())
        }
        Ok(status) => {
            diagnostics
                .write_all(&stdout)
                .with_context(|| format!("replaying captured stdout for {label}"))?;
            bail!("{label} failed (exit {})", status.code().unwrap_or(-1));
        }
        Err(error) => {
            diagnostics
                .write_all(&stdout)
                .with_context(|| format!("replaying captured stdout for {label}"))?;
            Err(error)
        }
    }
}

fn local_tool_root(root: &Path) -> PathBuf {
    root.join(".run")
        .join("tools")
        .join(format!("sqlx-cli-{VERSION}"))
}

fn sqlx_executable_path(root: &Path, windows: bool) -> PathBuf {
    let executable = if windows { "sqlx.exe" } else { "sqlx" };
    local_tool_root(root).join("bin").join(executable)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VersionProbe {
    Missing,
    Matches,
    Mismatch,
    Broken,
}

fn version_probe(executable: &Path) -> Result<VersionProbe> {
    Ok(match version_output(executable) {
        Ok(output) => {
            if matches_version(&String::from_utf8_lossy(&output)) {
                VersionProbe::Matches
            } else {
                VersionProbe::Mismatch
            }
        }
        Err(error) if child_process::is_timeout(&error) => return Err(error),
        Err(_) => VersionProbe::Broken,
    })
}

fn version_output(executable: &Path) -> Result<Vec<u8>> {
    let output = tempfile::NamedTempFile::new().context("creating SQLx version output file")?;
    let output_child = output
        .reopen()
        .context("opening SQLx version output file for child")?;
    let mut command = Command::new(executable);
    command.arg("--version").stdout(Stdio::from(output_child));
    let status = child_process::run(command, "sqlx-cli version check", VERSION_DEADLINE)?;
    if !status.success() {
        bail!(
            "sqlx-cli version check failed (exit {})",
            status.code().unwrap_or(-1)
        );
    }
    std::fs::read(output.path()).context("reading SQLx version output")
}

fn matches_version(output: &str) -> bool {
    output.trim().strip_prefix("sqlx-cli ") == Some(VERSION)
}

fn required_capabilities_available(root: &Path, executable: &Path) -> Result<bool> {
    let database_directory = tempfile::tempdir()?;
    let database_path = database_directory
        .path()
        .canonicalize()
        .context("resolving SQLx capability probe directory")?
        .join("sqlx-cli-capability.db");
    let config_path = database_directory.path().join("sqlx.toml");
    std::fs::write(&config_path, "[migrate]\nmigrations-dir = \"migrations\"\n")
        .context("writing SQLx capability probe config")?;
    let database_url = sqlite_url::from_path(&database_path);
    let arguments = [
        OsString::from("database"),
        OsString::from("create"),
        OsString::from("--config"),
        config_path.into_os_string(),
        OsString::from("--no-dotenv"),
        OsString::from("--database-url"),
        OsString::from(database_url),
        OsString::from("--connect-timeout"),
        OsString::from("10"),
    ];

    match run(
        root,
        executable,
        "sqlx-cli capability probe",
        &arguments,
        &[],
        CAPABILITY_DEADLINE,
    ) {
        Ok(()) => Ok(true),
        Err(error) if child_process::is_timeout(&error) => Err(error),
        Err(_) => Ok(false),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstallationAction {
    Keep,
    Install,
    Repair,
}

fn installation_action(
    version: VersionProbe,
    required_capabilities_available: bool,
) -> InstallationAction {
    match (version, required_capabilities_available) {
        (VersionProbe::Matches, true) => InstallationAction::Keep,
        (VersionProbe::Missing, _) => InstallationAction::Install,
        (VersionProbe::Matches | VersionProbe::Mismatch | VersionProbe::Broken, _) => {
            InstallationAction::Repair
        }
    }
}

fn install_arguments(root: &Path, action: InstallationAction) -> Vec<OsString> {
    let mut arguments = vec![
        OsString::from("install"),
        OsString::from("--locked"),
        OsString::from("sqlx-cli"),
        OsString::from("--version"),
        OsString::from(VERSION),
        OsString::from("--no-default-features"),
        OsString::from("--features"),
        OsString::from("sqlite,sqlx-toml"),
        OsString::from("--root"),
        local_tool_root(root).into_os_string(),
    ];
    if action == InstallationAction::Repair {
        arguments.push(OsString::from("--force"));
    }
    arguments
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, io::Write, path::Path, process::Command, time::Duration};

    use super::*;

    const FIXTURE_EXIT_CODE_ENVIRONMENT: &str = "PWF_XTASK_SQLX_FIXTURE_EXIT_CODE";
    const FIXTURE_STDOUT: &str = "Applied temporary fixture migration\n";
    const SUCCESS_SUMMARY: &str = "Prepared temporary SQLx database for query validation.";

    fn captured_stdout_fixture_command(exit_code: Option<&str>) -> Command {
        let executable = std::env::current_exe().expect("current test executable");
        let mut command = Command::new(executable);
        command.args([
            "--exact",
            "sqlx_cli::tests::captured_stdout_fixture",
            "--ignored",
        ]);
        if let Some(exit_code) = exit_code {
            command.env(FIXTURE_EXIT_CODE_ENVIRONMENT, exit_code);
        }
        command
    }

    #[test]
    fn contextual_success_replaces_child_stdout_with_summary() {
        let command = captured_stdout_fixture_command(None);
        let mut diagnostics = Vec::new();

        run_command_with_success_summary(
            command,
            "temporary database setup",
            Duration::from_secs(5),
            SUCCESS_SUMMARY,
            &mut diagnostics,
        )
        .unwrap();

        assert_eq!(
            String::from_utf8(diagnostics).unwrap(),
            format!("{SUCCESS_SUMMARY}\n")
        );
    }

    #[test]
    fn failure_replays_captured_stdout_before_returning_error() {
        let command = captured_stdout_fixture_command(Some("9"));
        let mut diagnostics = Vec::new();

        let error = run_command_with_success_summary(
            command,
            "temporary database setup",
            Duration::from_secs(5),
            SUCCESS_SUMMARY,
            &mut diagnostics,
        )
        .unwrap_err();

        assert!(
            diagnostics
                .windows(FIXTURE_STDOUT.len())
                .any(|window| window == FIXTURE_STDOUT.as_bytes())
        );
        assert!(
            error
                .to_string()
                .contains("temporary database setup failed (exit 9)")
        );
    }

    #[test]
    #[ignore = "captured stdout process fixture"]
    fn captured_stdout_fixture() {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(FIXTURE_STDOUT.as_bytes()).unwrap();
        stdout.flush().unwrap();
        if let Some(exit_code) = std::env::var_os(FIXTURE_EXIT_CODE_ENVIRONMENT) {
            let exit_code = exit_code
                .to_string_lossy()
                .parse()
                .expect("fixture exit code");
            std::process::exit(exit_code);
        }
    }

    #[test]
    fn tool_paths_are_repository_owned_and_platform_specific() {
        let root = Path::new("workspace");
        let tool_root = Path::new("workspace")
            .join(".run")
            .join("tools")
            .join("sqlx-cli-0.9.0");

        assert_eq!(local_tool_root(root), tool_root);
        assert_eq!(
            sqlx_executable_path(root, false),
            tool_root.join("bin").join("sqlx")
        );
        assert_eq!(
            sqlx_executable_path(root, true),
            tool_root.join("bin").join("sqlx.exe")
        );
    }

    #[test]
    fn installation_is_exact_repository_local_and_configured_sqlite_only() {
        assert_eq!(
            install_arguments(Path::new("workspace"), InstallationAction::Install),
            [
                OsString::from("install"),
                OsString::from("--locked"),
                OsString::from("sqlx-cli"),
                OsString::from("--version"),
                OsString::from("0.9.0"),
                OsString::from("--no-default-features"),
                OsString::from("--features"),
                OsString::from("sqlite,sqlx-toml"),
                OsString::from("--root"),
                Path::new("workspace")
                    .join(".run")
                    .join("tools")
                    .join("sqlx-cli-0.9.0")
                    .into_os_string(),
            ]
        );
    }

    #[test]
    fn wrong_broken_or_incapable_executable_requires_forced_local_repair() {
        for version in [VersionProbe::Mismatch, VersionProbe::Broken] {
            assert_eq!(
                installation_action(version, true),
                InstallationAction::Repair
            );
        }
        assert_eq!(
            installation_action(VersionProbe::Matches, false),
            InstallationAction::Repair
        );
        assert!(
            install_arguments(Path::new("workspace"), InstallationAction::Repair)
                .ends_with(&[OsString::from("--force")])
        );
    }

    #[test]
    fn missing_executable_installs_without_forcing() {
        assert_eq!(
            installation_action(VersionProbe::Missing, false),
            InstallationAction::Install
        );
        assert!(
            !install_arguments(Path::new("workspace"), InstallationAction::Install)
                .contains(&OsString::from("--force"))
        );
    }

    #[test]
    fn installed_version_must_match_canonical_output() {
        assert!(matches_version("sqlx-cli 0.9.0\n"));
        assert!(!matches_version("sqlx-cli-sqlx 0.9.1\n"));
        assert!(!matches_version("sqlx-cli 0.9.0 extra\n"));
    }
}
