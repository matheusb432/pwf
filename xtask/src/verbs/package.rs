use std::process::Command;

use anyhow::Result;

use crate::{paths, process};

pub(crate) fn run(allow_dirty: bool) -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let mut command = Command::new("cargo");
    command.current_dir(paths::repo_root()).args([
        "package",
        "--workspace",
        "--exclude",
        "xtask",
        "--locked",
        "--registry",
        "crates-io",
    ]);
    // Cargo caches temporary-registry sources by version, even across changed dry runs.
    command.env("CARGO_BUILD_BUILD_DIR", temporary.path());
    if allow_dirty {
        command.arg("--allow-dirty");
    }
    process::run("verify registry packages", &mut command)
}
