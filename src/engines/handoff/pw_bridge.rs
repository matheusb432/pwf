//! The cross-engine seam to the pending-work engine: allocating/closing/reopening
//! the pw item linked to a handoff, either by spawning the external
//! `--pending-work-script` allocator (tests inject `pw-stub.sh`) or, in production,
//! calling `pending_work::run_args` in-process. See AGENTS.md's cross-engine-seams
//! note before changing `add`'s output shape or any flag/field these functions
//! forward — this module compiles against either but breaks at runtime if missed.

use super::errors::HandoffError;
use crate::{cli::Args, config};

/// Extract the item id from `add` text output: `ADDED PWF TASK [<id>] …`.
pub(super) fn parse_added_id(text: &str) -> Option<String> {
    text.lines()
        .next()
        .and_then(|l| l.split('[').nth(1))
        .and_then(|s| s.split(']').next())
        .map(|s| s.to_string())
}

/// Spawn the external `--pending-work-script` allocator with the canonical
/// `add` protocol (`<script> add --config-path <cfg> --date <today>
/// <project> --continue-handoff`) and parse the id from its stdout text.
pub(super) fn spawn_pw_add(
    script: &str,
    cfg: &str,
    today: &str,
    project: &str,
) -> Result<String, HandoffError> {
    let output = std::process::Command::new(script)
        .args([
            "add",
            "--config-path",
            cfg,
            "--date",
            today,
            project,
            "--continue-handoff",
        ])
        .output()
        .map_err(|source| HandoffError::SubprocessSpawn {
            operation: "pw-add",
            script: script.to_string(),
            source,
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_added_id(stdout.trim()).ok_or_else(|| HandoffError::PwAddParse {
        stdout: stdout.to_string(),
    })
}

/// Spawn the external `--pending-work-script` allocator with the canonical
/// `check` protocol (`<script> check --config-path <cfg> --id <pw> --date <today>`).
pub(super) fn spawn_pw_check(
    script: &str,
    cfg: &str,
    pw: &str,
    today: &str,
    commits: &[String],
    review: bool,
) -> Result<(), HandoffError> {
    let mut argv: Vec<String> = ["check", "--config-path", cfg, "--id", pw, "--date", today]
        .iter()
        .map(|s| s.to_string())
        .collect();
    // Forward commit-range provenance + the review-task request (PWF-0017).
    for range in commits {
        argv.push("--commits".to_string());
        argv.push(range.clone());
    }
    if review {
        argv.push("--review".to_string());
    }
    std::process::Command::new(script)
        .args(&argv)
        .output()
        .map_err(|source| HandoffError::SubprocessSpawn {
            operation: "pw-check",
            script: script.to_string(),
            source,
        })?;
    Ok(())
}

/// In-process equivalent of spawn_pw_add for production (no --pending-work-script):
/// run the pending-work engine's `add <project> --continue-handoff` and read the id.
pub(super) fn inprocess_pw_add(
    args: &Args,
    today: &str,
    project: &str,
) -> Result<String, HandoffError> {
    let a = crate::cli::Args {
        action: Some("add".to_string()),
        date: Some(today.to_string()),
        config_path: args
            .config_path
            .clone()
            .or_else(config::default_config_path),
        project: Some(project.to_string()),
        continue_handoff: true,
        ..Default::default()
    };
    let out = crate::engines::pending_work::run_args(&a)
        .map_err(|message| HandoffError::PendingWork { message })?;
    parse_added_id(out.trim()).ok_or_else(|| HandoffError::PwAddParse {
        stdout: out.clone(),
    })
}

/// True when the linked pw item is still open. Probe failures (missing config or
/// notes dir) fall back to "open" so the check step keeps its existing error surface.
pub(super) fn pw_item_is_open(args: &Args, pw: &str) -> bool {
    let Some(path) = args
        .config_path
        .clone()
        .or_else(config::default_config_path)
    else {
        return true;
    };
    let Ok(cfg) = config::load(&path, None) else {
        return true;
    };
    crate::engines::pending_work::is_item_open(&cfg, pw).unwrap_or(true)
}

/// Spawn the external `--pending-work-script` allocator with the canonical
/// `reopen` protocol (`<script> reopen --config-path <cfg> --id <pw>`).
pub(super) fn spawn_pw_reopen(script: &str, cfg: &str, pw: &str) -> Result<(), HandoffError> {
    std::process::Command::new(script)
        .args(["reopen", "--config-path", cfg, "--id", pw])
        .output()
        .map_err(|source| HandoffError::SubprocessSpawn {
            operation: "pw-reopen",
            script: script.to_string(),
            source,
        })?;
    Ok(())
}

/// In-process equivalent of spawn_pw_reopen for production (no --pending-work-script).
/// `reopen` is idempotent, so an already-active linked item is a no-op skip.
pub(super) fn inprocess_pw_reopen(args: &Args, pw: &str) -> Result<(), HandoffError> {
    let a = crate::cli::Args {
        action: Some("reopen".to_string()),
        id: Some(pw.to_string()),
        config_path: args
            .config_path
            .clone()
            .or_else(config::default_config_path),
        ..Default::default()
    };
    crate::engines::pending_work::run_args(&a)
        .map_err(|message| HandoffError::PendingWork { message })?;
    Ok(())
}

/// In-process equivalent of spawn_pw_check for production (no --pending-work-script).
pub(super) fn inprocess_pw_check(args: &Args, today: &str, pw: &str) -> Result<(), HandoffError> {
    let a = crate::cli::Args {
        action: Some("check".to_string()),
        id: Some(pw.to_string()),
        date: Some(today.to_string()),
        config_path: args
            .config_path
            .clone()
            .or_else(config::default_config_path),
        // Forward commit-range provenance + the review-task request (PWF-0017).
        commits: args.commits.clone(),
        review: args.review,
        ..Default::default()
    };
    crate::engines::pending_work::run_args(&a)
        .map_err(|message| HandoffError::PendingWork { message })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_added_id_extracts_id() {
        assert_eq!(
            parse_added_id("ADDED PWF TASK [PWF-0001] pwf :: title"),
            Some("PWF-0001".to_string())
        );
        assert_eq!(parse_added_id("oops something went wrong"), None);
    }
}
