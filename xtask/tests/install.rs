#![cfg(unix)]

use std::{fs, os::unix::fs::PermissionsExt as _, process::Command};

#[test]
fn installs_cli_before_stopping_and_replacing_server() {
    let root = tempfile::tempdir().unwrap();
    let tools = root.path().join("tools");
    fs::create_dir(&tools).unwrap();
    let cargo = tools.join("cargo");
    fs::write(
        &cargo,
        r#"#!/bin/bash
set -eu
while [ "$#" -gt 0 ]; do
  case "$1" in
    --bin) binary="$2"; shift 2 ;;
    --root) destination="$2"; shift 2 ;;
    *) shift ;;
  esac
done
printf 'install %s\n' "$binary" >> "$INSTALL_TRACE"
mkdir -p "$destination/bin"
if [ "$binary" = pwf ]; then
  cat > "$destination/bin/pwf" <<'CLI'
#!/bin/bash
set -eu
printf 'pwf %s\n' "$*" >> "$INSTALL_TRACE"
if [ "$2" = install ]; then test -f "$PWF_TEST_ROOT/bin/pwf-server"; fi
CLI
  chmod +x "$destination/bin/pwf"
else
  touch "$destination/bin/pwf-server"
fi
"#,
    )
    .unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let install = root.path().join("cargo root");
    let trace = root.path().join("trace");
    let path = std::env::join_paths(
        std::iter::once(tools).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["install", "--root"])
        .arg(&install)
        .env("PATH", path)
        .env("INSTALL_TRACE", &trace)
        .env("PWF_TEST_ROOT", &install)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_to_string(trace).unwrap(),
        "install pwf\npwf server stop\ninstall pwf-server\npwf server install\n"
    );
}
