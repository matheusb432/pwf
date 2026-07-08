//! Confirmation text for interactive `pwf session` dispatches — builds the
//! frontmatter-style metadata block from the dispatch's operator context and
//! delegates rendering to the domain-agnostic [`crate::confirm_prompt`].

use super::DispatchOpts;
use crate::{
    confirm_prompt::{ConfirmationPrompt, Field},
    engines::pending_work::model::Item,
};

const CURRENT_TERMINAL_TARGET: &str = "current terminal";

pub(super) fn question(item: &Item, session: &str, opts: &DispatchOpts, agent: &str) -> String {
    let mode = DispatchMode::new(opts.inline, session);
    let fields = [
        Field::new("task_id", item.id.clone()),
        Field::new("title", item.session.clone()),
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
