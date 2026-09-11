use std::fs;

use super::support;

#[cfg(target_os = "linux")]
#[test]
fn doctor_formats_reports_and_keeps_json_unstyled() {
    let root = tempfile::tempdir().unwrap();
    let tools = systemctl_stub(root.path());
    for server in [false, true] {
        for (color, json) in [
            (None, false),
            (Some(false), false),
            (Some(true), false),
            (Some(true), true),
        ] {
            let binary = support::command().get_program().to_owned();
            let binary = if server {
                std::path::Path::new(&binary).with_file_name("pwf-server")
            } else {
                binary.into()
            };
            let mut command = std::process::Command::new(binary);
            command
                .env("PATH", &tools)
                .env("PWF_SERVICE_TEST_ROOT", root.path())
                .env("XDG_CONFIG_HOME", root.path().join("config"))
                .env("PWF_DATABASE_PATH", root.path().join("data/pwf.sqlite3"))
                .env("PWF_RUNTIME_DIR", root.path().join("runtime"))
                .env_remove("CLICOLOR_FORCE")
                .env_remove("NO_COLOR")
                .arg("doctor");
            if color.is_some() {
                command.env("CLICOLOR_FORCE", "1");
            }
            if color == Some(false) {
                command.env("NO_COLOR", "1");
            }
            if json {
                command.arg("--json");
            }
            let output = command.output().unwrap();
            assert_eq!(output.status.code(), Some(i32::from(!server)));
            assert!(output.stderr.is_empty());
            let text = String::from_utf8(output.stdout).unwrap();
            if json {
                let _: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert!(!text.contains('\x1b'));
            } else if color == Some(true) {
                assert!(text.contains("\x1b["), "{text}");
                assert!(!text.contains("**"), "{text}");
            } else {
                assert!(text.contains("**WARN** :: migrations\n  "), "{text}");
                assert!(text.contains("\n  Action: "), "{text}");
                assert!(!text.contains('\x1b'), "{text}");
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn doctor_reports_absent_service_as_json_without_creating_state() {
    let root = tempfile::tempdir().unwrap();
    let tools = systemctl_stub(root.path());
    let output = support::command()
        .env("PATH", tools)
        .env("PWF_SERVICE_TEST_ROOT", root.path())
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("PWF_DATABASE_PATH", root.path().join("data/pwf.sqlite3"))
        .env("PWF_RUNTIME_DIR", root.path().join("runtime"))
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let checks = report["checks"].as_array().unwrap();
    assert!(
        checks
            .iter()
            .any(|check| check["name"] == "service" && check["status"] == "fail")
    );
    assert!(
        checks
            .iter()
            .any(|check| check["name"] == "migrations" && check["status"] == "warning")
    );
    for directory in ["config", "data", "runtime"] {
        assert!(!root.path().join(directory).exists());
    }
}

#[test]
fn connection_failure_points_to_doctor_without_repeating_transport_causes() {
    let root = tempfile::tempdir().unwrap();
    let output = support::command()
        .env("PWF_RUNTIME_DIR", root.path().join("runtime"))
        .args(["project", "list"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("pwf doctor"), "{error}");
    assert!(!error.contains("transport error"), "{error}");
}

#[test]
fn incompatible_history_is_reported_offline_and_startup_returns_permanent_failure() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("pwf.sqlite3");
    drop(support::DatabaseFixture::new(path.clone()).unwrap());
    let mut bytes = fs::read(&path).unwrap();
    let descriptions = bytes
        .windows(b"initial".len())
        .enumerate()
        .filter_map(|(index, value)| (value == b"initial").then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 1);
    let start = descriptions[0];
    bytes[start..start + 7].copy_from_slice(b"changed");
    fs::write(&path, &bytes).unwrap();
    let server = std::path::Path::new(support::command().get_program())
        .with_file_name(format!("pwf-server{}", std::env::consts::EXE_SUFFIX));
    let report = std::process::Command::new(server)
        .env("PWF_DATABASE_PATH", &path)
        .env("PWF_RUNTIME_DIR", root.path().join("probe-runtime"))
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert_eq!(report.status.code(), Some(1));
    assert!(report.stderr.is_empty());
    let json: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert!(
        json["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["name"] == "migrations" && check["status"] == "fail")
    );
    assert!(!root.path().join("probe-runtime").exists());
    let startup = support::run_server_with_database(&path).unwrap();
    assert_eq!(startup.status.code(), Some(78));
    assert!(
        String::from_utf8(startup.stderr)
            .unwrap()
            .contains("migration 0 description")
    );
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[cfg(target_os = "linux")]
#[test]
fn doctor_succeeds_for_a_healthy_registered_server() {
    let root = tempfile::tempdir().unwrap();
    let database = support::DatabaseFixture::new(root.path().join("pwf.sqlite3")).unwrap();
    let tools = systemctl_stub(root.path());
    let mut command = database.command();
    let server = std::path::Path::new(command.get_program())
        .with_file_name("pwf-server")
        .canonicalize()
        .unwrap();
    let environment = command
        .get_envs()
        .filter_map(|(name, value)| {
            value.map(|value| format!("\"{}={}\"", name.to_string_lossy(), value.to_string_lossy()))
        })
        .collect::<Vec<_>>()
        .join(" ");
    fs::write(root.path().join("state"), "active").unwrap();
    fs::write(
        root.path().join("properties"),
        format!(
            "ExecStart={{ path={} ; argv[]={}}}\nEnvironment={environment}\n",
            server.display(),
            server.display()
        ),
    )
    .unwrap();
    let output = command
        .env("PATH", tools)
        .env("PWF_SERVICE_TEST_ROOT", root.path())
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    support::assert_success(&output, "doctor healthy server");
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|check| check["status"] == "pass"),
        "{report}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn offline_doctor_identifies_legacy_color_settings() {
    let root = tempfile::tempdir().unwrap();
    let settings = root.path().join("config/pwf/config.toml");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(&settings, "[colors]\nactive = \"#6495ED\"\n").unwrap();
    let server = std::path::Path::new(support::command().get_program())
        .with_file_name(format!("pwf-server{}", std::env::consts::EXE_SUFFIX));
    let output = std::process::Command::new(server)
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("APPDATA", root.path().join("config"))
        .env("PWF_DATABASE_PATH", root.path().join("missing.sqlite3"))
        .env("PWF_RUNTIME_DIR", root.path().join("runtime"))
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["name"] == "settings"
                && check["status"] == "fail"
                && check["action"].as_str().unwrap().contains("[colors.task]")),
        "{report}"
    );
    assert_eq!(
        fs::read_to_string(settings).unwrap(),
        "[colors]\nactive = \"#6495ED\"\n"
    );
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
    if [ -f "$PWF_SERVICE_TEST_ROOT/properties" ]; then cat "$PWF_SERVICE_TEST_ROOT/properties"; fi
    ;;
  show-environment) ;;
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
    let journal = tools.join("journalctl");
    fs::write(&journal, "#!/bin/sh\necho 'fixture startup failure'\n").unwrap();
    fs::set_permissions(journal, fs::Permissions::from_mode(0o755)).unwrap();
    std::env::join_paths(
        std::iter::once(tools).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap()
}

#[cfg(target_os = "linux")]
#[test]
fn doctor_reports_restart_loop_and_checks_the_registered_database() {
    let root = tempfile::tempdir().unwrap();
    let tools = systemctl_stub(root.path());
    let server = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/release/pwf-server")
        .canonicalize()
        .unwrap();
    let registered_database = root.path().join("service database.sqlite3");
    fs::write(root.path().join("state"), "activating").unwrap();
    fs::write(root.path().join("properties"), format!("SubState=auto-restart\nResult=exit-code\nExecMainStatus=78\nExecStart={{ path={} ; argv[]={}}}\nEnvironment=\"PWF_DATABASE_PATH={}\"\n", server.display(), server.display(), registered_database.display())).unwrap();
    let output = support::command()
        .env("PATH", tools)
        .env("PWF_SERVICE_TEST_ROOT", root.path())
        .env(
            "PWF_DATABASE_PATH",
            root.path().join("wrong-shell-database"),
        )
        .env("PWF_RUNTIME_DIR", root.path().join("runtime"))
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let checks = report["checks"].as_array().unwrap();
    assert!(
        checks.iter().any(|check| check["name"] == "service"
            && check["status"] == "fail"
            && check["detail"].as_str().unwrap().contains("auto-restart")),
        "{report}"
    );
    assert!(
        checks.iter().any(|check| check["name"] == "database"
            && check["detail"] == registered_database.to_str().unwrap()),
        "{report}"
    );
    assert!(!registered_database.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn installation_preflight_failure_preserves_service_registration() {
    use std::os::unix::fs::PermissionsExt as _;
    for (version, status, expected_error) in [
        (env!("CARGO_PKG_VERSION"), "fail", "migration 0 checksum"),
        ("incompatible", "pass", "version incompatible differs"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let tools = systemctl_stub(root.path());
        let binary = support::command().get_program().to_owned();
        let cli = root.path().join("pwf");
        fs::copy(binary, &cli).unwrap();
        let server = root.path().join("pwf-server");
        fs::write(&server, format!(r#"#!/bin/sh
test "$*" = 'doctor --json' || exit 42
echo '{{"version":"{version}","checks":[{{"name":"migrations","status":"{status}","detail":"SQLx migration 0 checksum does not match","action":"Use a compatible server release."}}]}}'
exit 1
"#)).unwrap();
        fs::set_permissions(&server, fs::Permissions::from_mode(0o755)).unwrap();
        let unit = root.path().join("config/systemd/user/pwf-server.service");
        fs::create_dir_all(unit.parent().unwrap()).unwrap();
        fs::write(&unit, "old registration").unwrap();
        let output = std::process::Command::new(cli)
            .env("PATH", tools)
            .env("PWF_SERVICE_TEST_ROOT", root.path())
            .env("XDG_CONFIG_HOME", root.path().join("config"))
            .env("NO_COLOR", "1")
            .args(["server", "install"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains(expected_error), "{error}");
        if status == "fail" {
            assert!(error.contains("**FAIL** :: migrations\n  "), "{error}");
        }
        assert_eq!(fs::read_to_string(unit).unwrap(), "old registration");
        assert!(!root.path().join("calls").exists());
    }
}
