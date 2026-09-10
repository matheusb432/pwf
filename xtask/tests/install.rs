#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt as _,
    process::{Command, Output},
};

#[test]
fn stages_and_checks_both_binaries_before_stopping_service() {
    let (root, output) = run_install("").unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(root.path().join("trace")).unwrap(),
        "stage binaries\npwf server install --check\npwf server stop\nplace binaries\npwf server install\n"
    );
}

#[test]
fn incompatible_update_preserves_installed_binaries_and_service() {
    let (root, output) = run_install("preflight").unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("preflight failed"));
    assert_eq!(
        fs::read_to_string(root.path().join("trace")).unwrap(),
        "stage binaries\npwf server install --check\n"
    );
    assert_original_binaries(root.path()).unwrap();
}

#[test]
fn failed_binary_placement_restores_previous_pair() {
    let (root, output) = run_install("placement").unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("previous binaries restored"));
    assert_eq!(
        fs::read_to_string(root.path().join("trace")).unwrap(),
        "stage binaries\npwf server install --check\npwf server stop\nplace binaries\n"
    );
    assert_original_binaries(root.path()).unwrap();
}

fn assert_original_binaries(root: &std::path::Path) -> anyhow::Result<()> {
    assert_eq!(
        fs::read_to_string(root.join("cargo root/bin/pwf"))?,
        "original CLI"
    );
    assert_eq!(
        fs::read_to_string(root.join("cargo root/bin/pwf-server"))?,
        "original server"
    );
    Ok(())
}

fn run_install(failure: &str) -> anyhow::Result<(tempfile::TempDir, Output)> {
    let root = tempfile::tempdir()?;
    let tools = root.path().join("tools");
    fs::create_dir(&tools)?;
    let cargo = tools.join("cargo");
    fs::write(
        &cargo,
        r#"#!/bin/bash
set -eu
binaries=()
while [ "$#" -gt 0 ]; do
  case "$1" in
    --bin) binaries+=("$2"); shift 2 ;;
    --root) destination="$2"; shift 2 ;;
    *) shift ;;
  esac
done
test "${binaries[*]}" = 'pwf pwf-server'
if [ "$destination" = "$PWF_TEST_ROOT" ]; then
  echo 'place binaries' >> "$INSTALL_TRACE"
  if [ "$INSTALL_FAILURE" = placement ]; then echo partial > "$destination/bin/pwf"; exit 1; fi
else
  echo 'stage binaries' >> "$INSTALL_TRACE"
fi
mkdir -p "$destination/bin"
cat > "$destination/bin/pwf" <<'CLI'
#!/bin/bash
set -eu
printf 'pwf %s\n' "$*" >> "$INSTALL_TRACE"
test -f "$(dirname "$0")/pwf-server"
if [ "$*" = 'server install --check' ] && [ "$INSTALL_FAILURE" = preflight ]; then
  echo 'SQLx migration 0 checksum does not match' >&2
  exit 1
fi
CLI
chmod +x "$destination/bin/pwf"
touch "$destination/bin/pwf-server"
"#,
    )?;
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755))?;
    let install = root.path().join("cargo root");
    fs::create_dir_all(install.join("bin"))?;
    fs::write(install.join("bin/pwf"), "original CLI")?;
    fs::write(install.join("bin/pwf-server"), "original server")?;
    let path = std::env::join_paths(std::iter::once(tools).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))?;
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["update", "--force", "--root"])
        .arg(&install)
        .env("PATH", path)
        .env("INSTALL_TRACE", root.path().join("trace"))
        .env("PWF_TEST_ROOT", &install)
        .env("INSTALL_FAILURE", failure)
        .output()?;
    Ok((root, output))
}
