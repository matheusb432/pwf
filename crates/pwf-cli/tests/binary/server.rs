use std::fs;

use super::support;

#[test]
fn server_help_is_available_without_a_server() {
    let temporary = tempfile::tempdir().unwrap();
    let output = support::command()
        .env("PWF_RUNTIME_DIR", temporary.path())
        .args(["server", "--help"])
        .output()
        .unwrap();
    support::assert_success(&output, "server help");
    let help = String::from_utf8(output.stdout).unwrap();
    for verb in ["install", "uninstall", "start", "stop", "restart", "status"] {
        assert!(help.contains(verb), "help omits {verb}");
    }
    assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 0);
}

#[cfg(target_os = "linux")]
#[test]
fn stopping_an_absent_service_needs_no_server_connection() {
    let root = tempfile::tempdir().unwrap();
    let tools = systemctl_stub(root.path());
    let output = support::command()
        .env("PATH", tools)
        .env("PWF_SERVICE_TEST_ROOT", root.path())
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("PWF_RUNTIME_DIR", root.path().join("runtime"))
        .args(["server", "stop"])
        .output()
        .unwrap();
    support::assert_success(&output, "stop absent server");
    assert!(!root.path().join("runtime").exists());
    assert!(
        !root
            .path()
            .join("config/systemd/user/pwf-server.service")
            .exists()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn installation_migrates_registration_and_uninstall_retains_data() {
    let root = tempfile::tempdir().unwrap();
    let database = support::DatabaseFixture::new(root.path().join("pwf.sqlite3")).unwrap();
    let path = systemctl_stub(root.path());
    let config = root.path().join("home/.config");
    let unit = config.join("systemd/user/pwf-server.service");
    fs::create_dir_all(unit.parent().unwrap()).unwrap();
    fs::write(&unit, "[Service]\nExecStart=/old/location/pwf-server\n").unwrap();
    fs::write(root.path().join("state"), "inactive").unwrap();
    let mut command = database.command();
    let installed = command
        .env("PATH", &path)
        .env("PWF_SERVICE_TEST_ROOT", root.path())
        .args(["server", "install"])
        .output()
        .unwrap();
    support::assert_success(&installed, "install service");
    let text = fs::read_to_string(&unit).unwrap();
    assert!(!text.contains("/old/location"));
    assert!(text.contains("/target/release/pwf-server\""));
    assert!(text.contains("WantedBy=default.target"));
    assert!(text.contains("TimeoutStopSec=12s"));
    assert!(text.contains("PWF_DATABASE_PATH="));
    let removed = database
        .command()
        .env("PATH", &path)
        .env("PWF_SERVICE_TEST_ROOT", root.path())
        .args(["server", "uninstall"])
        .output()
        .unwrap();
    support::assert_success(&removed, "uninstall service");
    assert!(!unit.exists());
    assert!(root.path().join("pwf.sqlite3").is_file());
    assert!(
        fs::read_to_string(root.path().join("calls"))
            .unwrap()
            .contains("daemon-reload")
    );
}

#[cfg(target_os = "linux")]
fn systemctl_stub(root: &std::path::Path) -> std::ffi::OsString {
    use std::os::unix::fs::PermissionsExt as _;
    let tools = root.join("tools");
    fs::create_dir(&tools).unwrap();
    let script = tools.join("systemctl");
    fs::write(
        &script,
        r#"#!/bin/bash
set -eu
printf '%s\n' "$*" >> "$PWF_SERVICE_TEST_ROOT/calls"
case "$2" in
  show)
    if [ -f "$PWF_SERVICE_TEST_ROOT/state" ]; then
      printf 'LoadState=loaded\nActiveState=%s\nMainPID=0\n' "$(cat "$PWF_SERVICE_TEST_ROOT/state")"
    else
      printf 'LoadState=not-found\nActiveState=inactive\nMainPID=0\n'
    fi
    ;;
  stop|enable) printf inactive > "$PWF_SERVICE_TEST_ROOT/state" ;;
  start) printf active > "$PWF_SERVICE_TEST_ROOT/state" ;;
  disable) rm "$PWF_SERVICE_TEST_ROOT/state" ;;
  daemon-reload) ;;
  *) exit 42 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(script, fs::Permissions::from_mode(0o755)).unwrap();
    std::env::join_paths(
        std::iter::once(tools).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap()
}
