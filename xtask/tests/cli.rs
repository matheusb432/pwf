//! Verifies the clap-derived verb surface.

use assert_cmd::Command;
use predicates::prelude::*;

fn xtask() -> Command {
    Command::cargo_bin("xtask").unwrap()
}

#[test]
fn forced_color_help_uses_cargo_palette() {
    xtask()
        .arg("--help")
        .env_remove("NO_COLOR")
        .env("CLICOLOR_FORCE", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains("\u{1b}["))
        .stdout(predicate::str::contains("36m"));
}

#[test]
fn help_lists_the_quality_verbs() {
    xtask()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("fmt"))
        .stdout(predicate::str::contains("fmt-check"))
        .stdout(predicate::str::contains("lint"))
        .stdout(predicate::str::contains("check"))
        .stdout(predicate::str::contains("fix"));
}

#[test]
fn fix_documents_forwarded_args() {
    xtask()
        .args(["fix", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("clippy"));
}

#[test]
fn fmt_check_help_is_formatting_only() {
    xtask()
        .args(["fmt-check", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("formatting"))
        .stdout(predicate::str::contains("lint").not());
}

#[test]
fn test_exposes_its_flags() {
    xtask()
        .args(["test", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--verbose"))
        .stdout(predicate::str::contains("--json"))
        .stdout(predicate::str::contains("--evidences"))
        .stdout(predicate::str::contains("--scope"))
        .stdout(predicate::str::contains("unit, e2e, all"))
        .stdout(predicate::str::contains("--e2e"))
        .stdout(predicate::str::contains("--all"));
}

#[test]
fn test_rejects_an_unknown_scope() {
    xtask()
        .args(["test", "--scope", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

#[test]
fn install_and_update_are_known_verbs() {
    xtask().args(["install", "--help"]).assert().success();
    xtask()
        .args(["update", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--dry"))
        .stdout(predicate::str::contains("--force"))
        .stdout(predicate::str::contains("full check preflight"));
}

#[test]
fn check_architecture_is_a_known_verb() {
    xtask()
        .args(["check-architecture", "--help"])
        .assert()
        .success();
}

#[test]
fn ship_is_a_known_verb() {
    xtask().args(["ship", "--help"]).assert().success();
}

#[test]
fn unknown_verb_is_rejected() {
    xtask()
        .arg("definitely-not-a-verb")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("unrecognized subcommand")
                .or(predicate::str::contains("unexpected argument")),
        );
}
