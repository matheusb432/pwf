use std::{
    fs::{self, File, OpenOptions},
    io::BufReader,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, bail, ensure};
use cargo_metadata::Message;
use clap::Args;
use serde_json::Value;

use crate::{paths::repo_root, process};

const TARGET: &str = "x86_64-pc-windows-msvc";
const BOOTSTRAP: &str = include_str!("test_windows/bootstrap.ps1");
const SMOKE: &str = include_str!("test_windows/smoke.ps1");
const MONITOR: &str = include_str!("test_windows/monitor.py");

#[derive(Args)]
pub(crate) struct Arguments {
    /// Existing dockur/windows container; its shared directory is discovered through Docker.
    #[arg(long, default_value = "winvm-rs")]
    container: String,
}

pub(crate) fn run(arguments: &Arguments) -> Result<()> {
    let root = repo_root();
    let artifacts = root.join(".artifacts/windows-smoke");
    fs::create_dir_all(&artifacts)?;
    let mut inspect = Command::new("docker");
    inspect.args([
        "inspect",
        "--format",
        r#"[{"State":{"Running":{{json .State.Running}}},"Mounts":{{json .Mounts}}}]"#,
        &arguments.container,
    ]);
    let inspection = capture(inspect, &artifacts.join("container.json"))?;
    let (shared, was_running) = container_state(&inspection)?;
    let lease = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(shared.join(".pwf-smoke.lock"))?;
    lease
        .try_lock()
        .context("another PWF smoke test is using this VM")?;
    let bundle = tempfile::Builder::new()
        .prefix("pwf-smoke-")
        .tempdir_in(shared)?;
    let run_id = bundle
        .path()
        .file_name()
        .context("test bundle has no directory name")?
        .to_str()
        .context("test bundle name is not UTF-8")?;
    let report = artifacts.join(run_id);
    fs::create_dir(&report)?;
    eprintln!("Windows smoke-test artifacts: {}", report.display());
    prepare_bundle(&root, bundle.path(), &report)?;

    let result = execute_guest(arguments, bundle.path(), run_id, &report);
    let shutdown = if was_running {
        Ok(())
    } else {
        vm_command(arguments, "down", Duration::from_secs(150))
    };
    let result = match (result, shutdown) {
        (Err(error), Err(shutdown)) => {
            Err(error.context(format!("VM shutdown also failed: {shutdown:#}")))
        }
        (result, Ok(())) => result,
        (Ok(()), Err(error)) => Err(error),
    };
    if let Err(error) = result {
        let retained = bundle.keep();
        eprintln!("Failed test bundle retained at {}", retained.display());
        return Err(error);
    }
    eprintln!("Windows smoke tests passed: {}", report.display());
    Ok(())
}

fn prepare_bundle(root: &Path, bundle: &Path, report: &Path) -> Result<()> {
    let mut build = Command::new("cargo");
    build.current_dir(root).args([
        "xwin",
        "build",
        "--release",
        "--target",
        TARGET,
        "-p",
        "pwf-app",
    ]);
    build
        .stdout(File::create(report.join("release-build.stdout.txt"))?)
        .stderr(File::create(report.join("release-build.stderr.txt"))?);
    eprintln!("Cross-compiling the Windows release binaries");
    process::run_bounded(
        "Windows release build (cargo-xwin and LLVM must be on PATH)",
        build,
        Duration::from_mins(20),
    )
    .with_context(|| format!("see build logs in {}", report.display()))?;
    let build_log = report.join("tests-build.jsonl");
    let mut build = Command::new("cargo");
    build
        .current_dir(root)
        .args([
            "xwin",
            "build",
            "--release",
            "--target",
            TARGET,
            "-p",
            "pwf-local-transport",
            "--tests",
            "--message-format=json",
        ])
        .stdout(File::create(&build_log)?)
        .stderr(File::create(report.join("tests-build.stderr.txt"))?);
    eprintln!("Cross-compiling the native named-pipe tests");
    process::run_bounded("Windows native test build", build, Duration::from_mins(20))?;
    let mut tests = Vec::new();
    for message in Message::parse_stream(BufReader::new(File::open(build_log)?)) {
        if let Message::CompilerArtifact(artifact) = message?
            && artifact.profile.test
            && let Some(executable) = artifact.executable
        {
            let name = executable
                .file_name()
                .context("test executable has no name")?;
            fs::copy(&executable, bundle.join(name))?;
            tests.push(name.to_owned());
        }
    }
    ensure!(
        !tests.is_empty(),
        "Cargo emitted no Windows test executables"
    );
    for name in ["pwf.exe", "pwf-server.exe"] {
        fs::copy(
            root.join("target").join(TARGET).join("release").join(name),
            bundle.join(name),
        )?;
    }
    fs::write(bundle.join("tests.json"), serde_json::to_vec(&tests)?)?;
    fs::write(bundle.join("smoke.ps1"), SMOKE)?;
    fs::write(bundle.join("bootstrap.ps1"), BOOTSTRAP)?;
    fs::write(
        bundle.join("launch.cmd"),
        concat!(
            "@echo off\r\n",
            "powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"%~dp0bootstrap.ps1\" -Bundle \"%~dp0.\" >\"%~dp0output\\bootstrap.stdout.txt\" 2>\"%~dp0output\\bootstrap.stderr.txt\"\r\n",
        ),
    )?;
    fs::set_permissions(bundle, fs::Permissions::from_mode(0o755))?;
    for entry in fs::read_dir(bundle)? {
        fs::set_permissions(entry?.path(), fs::Permissions::from_mode(0o644))?;
    }
    fs::create_dir(bundle.join("output"))?;
    fs::set_permissions(bundle.join("output"), fs::Permissions::from_mode(0o777))?;
    Ok(())
}

fn execute_guest(arguments: &Arguments, bundle: &Path, run_id: &str, report: &Path) -> Result<()> {
    vm_command(arguments, "up", Duration::from_secs(30))?;
    eprintln!("Waiting for Windows, then launching the guest test controller");
    let mut monitor = Command::new("docker");
    monitor.args([
        "exec",
        &arguments.container,
        "python3",
        "-c",
        MONITOR,
        run_id,
    ]);
    process::run_bounded("Windows desktop launch", monitor, Duration::from_mins(6))?;
    let deadline = Instant::now() + Duration::from_mins(6);
    let complete = bundle.join("output/complete.json");
    while !complete.is_file() {
        if Instant::now() >= deadline {
            copy_output(bundle, report)?;
            bail!(
                "Windows did not finish within six minutes; inspect {} and the VM desktop",
                report.display()
            );
        }
        thread::sleep(Duration::from_millis(500));
    }
    copy_output(bundle, report)?;
    let bytes = fs::read(&complete)?;
    let result: Value =
        serde_json::from_slice(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes))?;
    ensure!(
        result["run_id"] == run_id,
        "guest result belongs to another run"
    );
    ensure!(
        result["passed"] == true,
        "Windows smoke tests failed: {result:#}"
    );
    eprintln!("{result:#}");
    Ok(())
}

fn copy_output(bundle: &Path, report: &Path) -> Result<()> {
    for entry in fs::read_dir(bundle.join("output"))? {
        let entry = entry?;
        ensure!(
            entry.file_type()?.is_file(),
            "guest output must contain regular files"
        );
        fs::copy(entry.path(), report.join(entry.file_name()))?;
    }
    Ok(())
}

fn vm_command(arguments: &Arguments, verb: &str, timeout: Duration) -> Result<()> {
    let mut command = Command::new("winvm-rs");
    command.arg(verb).env("WINVM_RS_NAME", &arguments.container);
    process::run_bounded("winvm-rs lifecycle command", command, timeout)
}

fn capture(mut command: Command, path: &Path) -> Result<Vec<u8>> {
    command.stdout(Stdio::from(File::create(path)?));
    process::run_bounded("VM status inspection", command, Duration::from_secs(15))?;
    Ok(fs::read(path)?)
}

fn container_state(bytes: &[u8]) -> Result<(PathBuf, bool)> {
    let inspection: Value = serde_json::from_slice(bytes)?;
    let container = inspection
        .as_array()
        .and_then(|items| items.first())
        .context("Docker returned no container")?;
    let running = container["State"]["Running"]
        .as_bool()
        .context("Docker omitted container state")?;
    let mounts = container["Mounts"]
        .as_array()
        .context("Docker omitted container mounts")?;
    let shared = mounts
        .iter()
        .find(|mount| mount["Destination"] == "/shared" && mount["Type"] == "bind")
        .and_then(|mount| mount["Source"].as_str())
        .context("dockur container must bind a host directory at /shared")?;
    let shared = PathBuf::from(shared);
    ensure!(
        shared.is_absolute() && shared.is_dir(),
        "Docker shared directory must exist and be absolute"
    );
    Ok((shared, running))
}
