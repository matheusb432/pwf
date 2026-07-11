//! The cross-engine seam to the pending-work engine: allocating the pw item
//! linked to a new handoff, either by spawning the external
//! `--pending-work-script` allocator (tests inject `pw-stub.sh`) or, in production,
//! reusing the pending-work application request path in-process — the shared
//! `AddPendingWorkItem` command build + mediator sender (`inprocess_pw_add`).
//! `done`/`cancel`/`reopen`/`refresh` are retired (PWF-0117); closing/reopening
//! a handoff-tagged pw item mirrors from the pw verbs instead, via
//! `engines/handoff/mirror.rs`, called directly from the pending-work actions
//! (`engines/pending_work/actions/{done,reopen,remove}.rs`) and
//! `pending_work::run::run_add`. See AGENTS.md's cross-engine-seams note
//! before changing `add`'s output shape or any flag/field these functions
//! forward — this module compiles against either but breaks at runtime if
//! missed.

use cqrsy::Sender;

use super::errors::HandoffError;
use crate::{cli::Args, config};

/// Extract the item id from `add` text output: `ADDED PWF TASK [<id>] …`.
/// Only the external `--pending-work-script` allocator (`spawn_pw_add`) still
/// needs this — the in-process path (`inprocess_pw_add`) consumes the typed
/// `AddedItem` directly from the shared mediator path.
pub(super) fn parse_added_id(text: &str) -> Option<String> {
    text.lines()
        .next()
        .and_then(|l| l.split('[').nth(1))
        .and_then(|s| s.split(']').next())
        .map(std::string::ToString::to_string)
}

/// Spawn the external `--pending-work-script` allocator with the canonical
/// `add` protocol (`<script> add --config-path <cfg> --date <today>
/// <project> --tag handoff --continue-handoff`) and parse the id from its
/// stdout text.
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
            "--tag",
            pwf_domain::pending_work::HANDOFF_TAG,
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

/// In-process equivalent of `spawn_pw_add` for production (no --pending-work-script):
/// build the same `AddPendingWorkItem` command `pwf add <project> --continue-handoff`
/// would, send it through the shared pending-work mediator, and take the id
/// directly — no stdout text to parse, so a malformed id can no longer slip
/// past a text-shape check unnoticed.
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
        tag: vec![pwf_domain::pending_work::HANDOFF_TAG.to_string()],
        ..Default::default()
    };
    let (cfg, command) =
        crate::engines::pending_work::add_command_from_args(&a).map_err(|error| {
            HandoffError::PendingWork {
                message: error.to_string(),
            }
        })?;
    let mediator = crate::engines::pending_work::pending_work_mediator(&cfg);
    let result = mediator.send_now(command);
    match &result {
        Ok(added) => crate::engines::pending_work::emit_created_section_diagnostic(added),
        Err(error) => {
            crate::engines::pending_work::emit_created_section_diagnostic_for_error(error);
        }
    }
    let item = result.map_err(|error| HandoffError::PendingWork {
        message: error.to_string(),
    })?;
    Ok(item.id)
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    #[test]
    fn parse_added_id_extracts_id() {
        assert_eq!(
            parse_added_id("ADDED PWF TASK [PWF-0001] pwf :: title"),
            Some("PWF-0001".to_string())
        );
        assert_eq!(parse_added_id("oops something went wrong"), None);
    }

    /// A bad/nonexistent `--config-path` must surface as `HandoffError::PendingWork`
    /// — the seam's only error mapping — rather than panicking or silently
    /// swallowing the underlying `PendingWorkError`. The message comes straight
    /// from `PendingWorkError`'s `Display` (see `query.rs`'s
    /// `load_config_returns_typed_config_error_with_legacy_display`).
    #[test]
    fn inprocess_pw_add_maps_bad_config_path_to_pending_work_error() {
        let stage = crate::engines::handoff::test_support::tempdir();
        let missing_config = stage.path().join("missing_config.json");
        assert!(!missing_config.exists());
        let args = crate::cli::Args {
            config_path: Some(missing_config.to_string_lossy().into_owned()),
            ..Default::default()
        };

        let err = inprocess_pw_add(&args, "2026-01-01", "test-project").unwrap_err();

        assert_matches!(err, HandoffError::PendingWork { ref message } if message.contains("Pending work config not found"));
    }

    // The conformance corpus (pw-stub.sh) only exercises the external
    // `--pending-work-script` path; a break in the in-process `add` seam is
    // invisible to it (PWF project memory: "conformance stub masks protocol
    // break"). This drives the real bridge wrapper (`inprocess_pw_add`) —
    // synthetic-Args build, shared `AddPendingWorkItem` command construction,
    // mediator send, and id extraction
    // included — and asserts on real returned/on-disk data rather than
    // pre-rendered text, so a wrapper-confined regression (e.g. returning the
    // wrong `AddedItem` field as the id) fails here.
    #[test]
    fn inprocess_pw_add_seam_resolves_to_open_item_on_disk() {
        let stage = crate::engines::handoff::test_support::tempdir();
        let notes = stage.path().join("notes");
        let repo = stage.path().join("repo");
        let handoff_dir = repo.join("docs/handoffs");
        std::fs::create_dir_all(&handoff_dir).unwrap();
        std::fs::write(
            handoff_dir.join("2026-01-01-managed-flow.md"),
            crate::engines::handoff::scaffold("Managed Flow", "test-project", "2026-01-01", None),
        )
        .unwrap();

        let cfg_path = stage.path().join("config.json");
        std::fs::write(
            &cfg_path,
            format!(
                r#"{{ "notesDir": "{}", "projects": {{ "test-project": "{}" }}, "prefixes": {{ "test-project": "TST" }} }}"#,
                notes.to_string_lossy().replace('\\', "/"),
                repo.to_string_lossy().replace('\\', "/"),
            ),
        )
        .unwrap();

        let args = crate::cli::Args {
            config_path: Some(cfg_path.to_string_lossy().into_owned()),
            ..Default::default()
        };

        let id = inprocess_pw_add(&args, "2026-01-01", "test-project").unwrap();

        assert!(id.starts_with("TST-"), "unexpected id: {id}");

        let cfg = crate::config::load(&cfg_path.to_string_lossy(), None).unwrap();
        assert!(
            crate::engines::pending_work::is_item_open(&cfg, &id).unwrap(),
            "returned id should resolve as an open pending-work item"
        );

        let note_path = crate::engines::pending_work::project_dir(
            cfg.notes_dir_for("test-project"),
            "test-project",
        )
        .join(format!("{id}.md"));
        let note = std::fs::read_to_string(&note_path).unwrap();
        assert!(
            note.contains("status: active"),
            "expected an active item note, got: {note}"
        );
    }
}
