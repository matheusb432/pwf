//! Top-level dispatcher for `pwf handoff <action>`. `done`/`cancel`/`reopen`/
//! `refresh` are retired (PWF-0117, `main::retired_handoff_verb`) — a
//! handoff-tagged pw item mirrors those operations from the pw verbs instead,
//! so only `add`/`list` remain reachable here.

use super::{
    actions::{invoke_add, invoke_list},
    errors::HandoffError,
    paths::repo_root_typed,
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
        "add" => invoke_add(&root, args),
        "list" => Ok(invoke_list(&root, args)),
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
            action: Some("add".to_string()),
            repo_root: Some(root.path().to_string_lossy().into_owned()),
            ..Default::default()
        };

        let err = run_typed(&args).unwrap_err();

        assert_matches!(err, HandoffError::MissingTitle);
        assert_eq!(err.to_string(), "--title is required for add.");
    }

    #[test]
    fn run_typed_treats_retired_done_as_unknown_action() {
        // `done` is retired pre-parse (`main::retired_handoff_verb`); this only
        // covers a caller that reaches `run_typed` directly, bypassing that guard.
        let root = tempdir();
        let args = Args {
            action: Some("done".to_string()),
            repo_root: Some(root.path().to_string_lossy().into_owned()),
            ..Default::default()
        };

        let err = run_typed(&args).unwrap_err();

        assert_matches!(err, HandoffError::UnknownAction { ref action } if action == "done");
        assert_eq!(err.to_string(), "unknown handoff action: done");
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
