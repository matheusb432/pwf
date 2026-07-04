//! Top-level dispatcher for `pwf handoff <action>`.

use super::{
    actions::{complete_handoff, invoke_list, invoke_new, reopen_handoff},
    errors::HandoffError,
    ledger::{archive_stranded, refresh_ledger_typed},
    paths::{handoff_paths, repo_root_typed},
};
use crate::cli::Args;

pub fn run(args: &Args) -> Result<String, String> {
    run_typed(args).map_err(|e| e.to_string())
}

pub(super) fn run_typed(args: &Args) -> Result<String, HandoffError> {
    let action = args
        .action
        .as_deref()
        .ok_or(HandoffError::MissingSubcommand)?;
    let root = repo_root_typed(args)?;

    match action {
        "refresh" => {
            let (archived, conflicts) = archive_stranded(&handoff_paths(&root))?;
            let (_ledger, _count) = refresh_ledger_typed(&root)?;
            let mut out = "LEDGER refreshed.".to_string();
            if archived > 0 {
                out.push_str(&format!("\n  archived {archived} stranded handoff(s)"));
            }
            for c in &conflicts {
                out.push_str(&format!(
                    "\n  conflict: {c} already exists in archived/ \u{2014} left in place"
                ));
            }
            Ok(out)
        }
        "new" => invoke_new(&root, args),
        "done" => complete_handoff(&root, "done", args),
        "cancel" => complete_handoff(&root, "cancelled", args),
        "reopen" => reopen_handoff(&root, args),
        "list" => invoke_list(&root, args),
        other => Err(HandoffError::UnknownAction {
            action: other.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;
    use crate::engines::handoff::test_support::tempdir;

    #[test]
    fn run_typed_preserves_missing_title_text_with_variant() {
        let root = tempdir();
        let args = Args {
            action: Some("new".to_string()),
            repo_root: Some(root.path().to_string_lossy().into_owned()),
            ..Default::default()
        };

        let err = run_typed(&args).unwrap_err();

        assert_matches!(err, HandoffError::MissingTitle);
        assert_eq!(err.to_string(), "--title is required for new.");
    }

    #[test]
    fn run_typed_preserves_missing_id_text_with_action_field() {
        let root = tempdir();
        let args = Args {
            action: Some("done".to_string()),
            repo_root: Some(root.path().to_string_lossy().into_owned()),
            ..Default::default()
        };

        let err = run_typed(&args).unwrap_err();

        assert_matches!(err, HandoffError::MissingId { ref action } if action == "done");
        assert_eq!(err.to_string(), "--id is required for done.");
    }

    #[test]
    fn run_typed_preserves_unknown_action_text_with_action_field() {
        let root = tempdir();
        let args = Args {
            action: Some("wat".to_string()),
            repo_root: Some(root.path().to_string_lossy().into_owned()),
            ..Default::default()
        };

        let err = run_typed(&args).unwrap_err();

        assert_matches!(err, HandoffError::UnknownAction { ref action } if action == "wat");
        assert_eq!(err.to_string(), "unknown handoff action: wat");
    }
}
