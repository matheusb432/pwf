//! Confirmation text for interactive `pwf session` dispatches — builds the
//! frontmatter-style metadata block from the dispatch's operator context and
//! delegates rendering to the domain-agnostic [`crate::confirm_prompt`].

use super::DispatchOpts;
use crate::{
    confirm_prompt::{ConfirmationPrompt, Field},
    engines::pending_work::model::Item,
};

const CURRENT_TERMINAL_TARGET: &str = "current terminal";
/// Shown for legacy inline items whose note carries no `created` frontmatter.
const UNKNOWN_CREATED: &str = "(unknown)";

pub(super) fn question(item: &Item, session: &str, opts: &DispatchOpts, agent: &str) -> String {
    let mode = DispatchMode::new(opts.inline, session);
    let fields = [
        Field::new("task_id", item.id.clone()),
        Field::new("title", item.session.clone()),
        Field::new(
            "created",
            item.created
                .clone()
                .unwrap_or_else(|| UNKNOWN_CREATED.to_string()),
        ),
        Field::new("mode", mode.label()),
        Field::new("agent", agent),
        Field::new(
            "autonomy",
            Enabled::from(opts.launch.auto.into_inner()).label(),
        ),
        Field::new(
            "worktree",
            Enabled::from(opts.launch.worktree.into_inner()).label(),
        ),
        Field::new("target", mode.target()),
    ];
    ConfirmationPrompt::new(
        "Confirm session dispatch",
        &fields,
        "Proceed with session dispatch?",
    )
    .to_string()
}

#[derive(Clone, Copy)]
enum DispatchMode<'a> {
    Inline,
    Zellij { session: &'a str },
}

impl<'a> DispatchMode<'a> {
    fn new(inline: bool, session: &'a str) -> Self {
        if inline {
            DispatchMode::Inline
        } else {
            DispatchMode::Zellij { session }
        }
    }

    fn label(self) -> &'static str {
        match self {
            DispatchMode::Inline => "inline",
            DispatchMode::Zellij { .. } => "zellij",
        }
    }

    fn target(self) -> String {
        match self {
            DispatchMode::Inline => CURRENT_TERMINAL_TARGET.to_string(),
            DispatchMode::Zellij { session } => format!("zellij session {session}"),
        }
    }
}

#[derive(Clone, Copy)]
enum Enabled {
    Yes,
    No,
}

impl Enabled {
    fn label(self) -> &'static str {
        match self {
            Enabled::Yes => "yes",
            Enabled::No => "no",
        }
    }
}

impl From<bool> for Enabled {
    fn from(value: bool) -> Self {
        if value { Enabled::Yes } else { Enabled::No }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::{Agent, ColorChoice},
        engines::pending_work::launch::{Auto, LaunchPolicy, Worktree},
    };

    #[test]
    fn question_renders_dispatch_context_metadata_without_the_prompt_body() {
        let mut item = Item::default_for_test("PWF-0001", "dispatch me");
        item.created = Some("2026-07-01".to_string());
        item.prompt = "do work".to_string();
        item.note = "do work".to_string();
        let opts = DispatchOpts {
            color: ColorChoice::Never,
            assume_yes: false,
            inline: true,
            launch: LaunchPolicy {
                worktree: Worktree::from(true),
                auto: Auto::from(true),
            },
            agent: Agent::Codex,
            model_override: None,
        };

        let out = question(&item, "pwf", &opts, "codex");

        assert!(out.contains("# Confirm session dispatch"));
        assert!(out.contains("task_id: PWF-0001"));
        assert!(out.contains("title: dispatch me"));
        assert!(out.contains("created: 2026-07-01"));
        assert!(out.contains("mode: inline"));
        assert!(out.contains("agent: codex"));
        assert!(out.contains("autonomy: yes"));
        assert!(out.contains("worktree: yes"));
        assert!(out.contains("target: current terminal"));
        assert!(
            !out.contains("do work"),
            "confirmation must not flood the TUI with the prompt body: {out}"
        );
    }

    #[test]
    fn zellij_mode_targets_the_named_session_and_falls_back_on_missing_created() {
        let item = Item::default_for_test("PWF-0001", "dispatch me");
        let opts = DispatchOpts {
            color: ColorChoice::Never,
            assume_yes: false,
            inline: false,
            launch: LaunchPolicy {
                worktree: Worktree::from(false),
                auto: Auto::from(false),
            },
            agent: Agent::Claude,
            model_override: None,
        };

        let out = question(&item, "pwf", &opts, "claude");

        assert!(out.contains("mode: zellij"));
        assert!(out.contains("target: zellij session pwf"));
        assert!(out.contains("autonomy: no"));
        assert!(out.contains("worktree: no"));
        assert!(out.contains("created: (unknown)"));
    }
}
