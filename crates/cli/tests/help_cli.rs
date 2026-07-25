//! Checks binary help dispatch through Cargo's injected `CARGO_BIN_EXE_pwf` path.
use std::{fs, process::Command};

#[path = "support/database.rs"]
mod database;

use database::DatabaseFixture;

// Help and clap rejections exit before application dispatch, so they do not open the database.
fn database_independent_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pwf"))
}

fn run(args: &[&str]) -> (String, bool) {
    let out = database_independent_command()
        .args(args)
        .output()
        .expect("run pwf");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.success(),
    )
}

#[test]
fn forced_color_help_uses_cargo_palette() {
    let out = database_independent_command()
        .arg("--help")
        .env_remove("NO_COLOR")
        .env("CLICOLOR_FORCE", "1")
        .output()
        .expect("run pwf");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success(), "stdout: {stdout}");
    assert!(
        stdout.contains("\u{1b}["),
        "help should contain ANSI styling: {stdout}"
    );
    assert!(
        stdout.contains("36m"),
        "help should use Cargo's cyan palette: {stdout}"
    );
}

#[test]
fn no_color_help_strips_ansi_styling() {
    let out = database_independent_command()
        .arg("--help")
        .env_remove("CLICOLOR_FORCE")
        .env("NO_COLOR", "1")
        .output()
        .expect("run pwf");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success(), "stdout: {stdout}");
    assert!(
        !stdout.contains("\u{1b}["),
        "help should be unstyled: {stdout}"
    );
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

fn stage_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("pwcli_{}", nanos()));
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn default_engine_treats_next_arg_as_project() {
    let stage = stage_dir();
    let notes = stage.join("notes");
    let foo = notes.join("foo-bar");
    let config = notes.join("config-handler");
    fs::create_dir_all(&foo).unwrap();
    fs::create_dir_all(&config).unwrap();
    fs::write(
        foo.join("FOO-0001.md"),
        "---\nid: FOO-0001\nstatus: active\ntitle: tray gui\nproject: foo-bar\ncreated: 2026-01-01\n---\n\nadd startup toggle\n",
    )
    .unwrap();
    fs::write(
        foo.join("foo-bar.md"),
        "---\nid: foo\ntitle: foo-bar\n---\n\n- [ ] [[FOO-0001|tray gui]]\n",
    )
    .unwrap();
    fs::write(
        config.join("CFG-0001.md"),
        "---\nid: CFG-0001\nstatus: active\ntitle: config task\nproject: config-handler\ncreated: 2026-01-01\n---\n\nfix config\n",
    )
    .unwrap();
    fs::write(
        config.join("config-handler.md"),
        "---\nid: cfg\ntitle: config-handler\n---\n\n- [ ] [[CFG-0001|config task]]\n",
    )
    .unwrap();
    let database = DatabaseFixture::new(stage.join("projects.sqlite3"));
    database.add_directory_project(
        "CFG",
        "config-handler",
        std::path::Path::new("/config"),
        &config,
    );
    database.add_directory_project("FOO", "foo-bar", std::path::Path::new("/foo"), &foo);

    let out = database
        .command()
        .args(["config-handler"])
        .output()
        .expect("run pwf");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    assert!(stdout.contains("CFG-0001 :: config task"));
    assert!(!stdout.contains("FOO-0001"), "got: {stdout}");
}

#[test]
fn list_is_an_alias_for_help() {
    let (a, _) = run(&["--list"]);
    let (b, _) = run(&["--help"]);
    assert_eq!(a, b, "--list output should equal --help output");
}

#[test]
fn project_bare_command_prints_scoped_help() {
    let (bare, bare_ok) = run(&["project"]);
    let (explicit, explicit_ok) = run(&["project", "--help"]);

    assert!(bare_ok, "bare pwf project should exit 0");
    assert!(explicit_ok, "pwf project --help should exit 0");
    assert_eq!(bare, explicit, "bare project should print project help");
}
