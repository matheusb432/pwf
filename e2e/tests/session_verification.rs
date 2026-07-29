#![cfg(unix)]

use std::fs;

use assert_cmd::prelude::OutputAssertExt as _;

use crate::common::SessionFixture;

#[test]
fn verify_probes_and_previews_the_selected_provider() {
    let fixture = SessionFixture::new();

    let assertion = fixture
        .database
        .command()
        .args(["verify", "PWF-0001", "--agent", "codex"])
        .env("PATH", &fixture.child_path)
        .assert()
        .success();

    let stdout = String::from_utf8(assertion.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("codex: available (codex-cli fixture 1.0)"),
        "{stdout}"
    );
    assert!(stdout.contains("command: codex resume "), "{stdout}");
    assert!(!stdout.contains("claude:"), "{stdout}");
}

#[test]
fn broken_effort_tier_catalog_stops_before_dispatch() {
    let fixture = SessionFixture::new();
    fixture
        .database
        .command()
        .args(["update", "PWF-0001", "--effort", "highest"])
        .assert()
        .success();
    let missing_tiers = fixture.directory().join("does-not-exist.toml");

    fixture
        .database
        .command()
        .args(["session", "--id", "PWF-0001", "--agent", "claude", "--yes"])
        .env("PATH", &fixture.child_path)
        .env("TMUX_STUB_LOG", &fixture.tmux_log_path)
        .env("PWF_MODEL_TIERS", missing_tiers)
        .assert()
        .failure();

    assert!(
        !fixture.tmux_log_path.exists()
            || fs::read_to_string(&fixture.tmux_log_path)
                .unwrap()
                .is_empty()
    );
}
