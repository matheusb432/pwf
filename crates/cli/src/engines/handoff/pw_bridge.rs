//! Allocates the pending-work item linked to a new handoff.
//!
//! External allocators use the raw `ADDED PWF TASK [<id>]` stdout protocol. Production calls
//! the same add operation in-process and consumes its typed result. Preserve both seams.

use super::errors::HandoffError;
use crate::{cli::EngineArgs, config};

/// Extracts an item ID from the external allocator's `ADDED PWF TASK [<id>]` output.
pub(super) fn parse_added_id(text: &str) -> Option<String> {
    text.lines()
        .next()
        .and_then(|l| l.split('[').nth(1))
        .and_then(|s| s.split(']').next())
        .map(std::string::ToString::to_string)
}

/// Runs the external allocator with the canonical add argv and parses its stdout ID.
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

/// Runs the equivalent add operation in-process and returns its typed ID.
pub(super) fn inprocess_pw_add(
    args: &EngineArgs,
    today: &str,
    project: &str,
) -> Result<String, HandoffError> {
    let a = crate::cli::EngineArgs {
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
    let (store, registry, command) = crate::engines::pending_work::add_command_from_args(&a)
        .map_err(|error| HandoffError::PendingWork {
            message: error.to_string(),
        })?;
    let result = pwf_application::pending_work::add::execute(command, &store, &registry);
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

    #[test]
    fn inprocess_pw_add_maps_bad_config_path_to_pending_work_error() {
        let stage = crate::engines::handoff::test_support::tempdir();
        let missing_config = stage.path().join("missing_config.json");
        assert!(!missing_config.exists());
        let args = crate::cli::EngineArgs {
            config_path: Some(missing_config.to_string_lossy().into_owned()),
            ..Default::default()
        };

        let err = inprocess_pw_add(&args, "2026-01-01", "test-project").unwrap_err();

        assert_matches!(err, HandoffError::PendingWork { ref message } if message.contains("Pending work config not found"));
    }

    // The shell-stub corpus covers only the external allocator; this test exercises the in-process
    // seam.
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

        let args = crate::cli::EngineArgs {
            config_path: Some(cfg_path.to_string_lossy().into_owned()),
            ..Default::default()
        };

        let id = inprocess_pw_add(&args, "2026-01-01", "test-project").unwrap();

        assert!(id.starts_with("TST-"), "unexpected id: {id}");

        let cfg = crate::config::load(&cfg_path.to_string_lossy(), None).unwrap();
        let store = crate::engines::pending_work::store_for(&cfg);
        let projects = crate::engines::pending_work::project_registry(&cfg);
        assert!(
            crate::engines::pending_work::is_item_open(&store, &projects, &id).unwrap(),
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
