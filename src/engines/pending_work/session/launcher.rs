//! How an agent is run inside the new zellij tab. `ClaudeLauncher` is the only
//! impl today; codex slots in behind the same trait (PWF-0068).

use crate::engines::pending_work::launch::new_launch_prompt;
use crate::engines::pending_work::model::Item;

/// Builds the tab name and the argv that runs an agent in the new zellij tab.
pub(in crate::engines::pending_work) trait AgentLauncher {
    /// Name for the new tab (the canonical item id).
    fn tab_name(&self, item: &Item) -> String;
    /// argv after `zellij … new-tab … --`, e.g. `["claude","--name",<title>,<prompt>]`.
    fn argv(&self, item: &Item) -> Vec<String>;
}

/// Launches Claude Code with the item's launch prompt.
pub(in crate::engines::pending_work) struct ClaudeLauncher;

impl AgentLauncher for ClaudeLauncher {
    fn tab_name(&self, item: &Item) -> String {
        item.id.clone()
    }

    fn argv(&self, item: &Item) -> Vec<String> {
        vec![
            "claude".to_string(),
            "--name".to_string(),
            item.session.clone(),
            new_launch_prompt(item),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::pending_work::model::Item;

    #[test]
    fn claude_launcher_builds_named_argv_with_launch_prompt() {
        let item = Item {
            prompt: "do the thing".to_string(),
            ..Item::default_for_test("PWF-0038", "zellij dispatches")
        };
        let l = ClaudeLauncher;
        assert_eq!(l.tab_name(&item), "PWF-0038");
        let argv = l.argv(&item);
        assert_eq!(argv[0], "claude");
        assert_eq!(argv[1], "--name");
        assert_eq!(argv[2], "zellij dispatches");
        // argv[3] is new_launch_prompt(item) — a single element, no splitting.
        assert!(argv[3].contains("do the thing"));
        assert_eq!(argv.len(), 4);
    }
}
