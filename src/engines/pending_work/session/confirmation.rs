//! Confirmation text for interactive `pwf session` dispatches.

use super::DispatchOpts;
use crate::engines::pending_work::model::Item;

const CURRENT_TERMINAL_TARGET: &str = "current terminal";

pub(super) fn question(item: &Item, session: &str, opts: DispatchOpts, agent: &str) -> String {
    let details = ConfirmationDetails::new(item, session, opts, agent);
    format!(
        "# Confirm session dispatch\n\n{}\n\nProceed with session dispatch?",
        details.table()
    )
}

struct ConfirmationDetails<'a> {
    id: &'a str,
    title: &'a str,
    mode: DispatchMode<'a>,
    agent: &'a str,
    autonomy: Enabled,
    worktree: Enabled,
}

impl<'a> ConfirmationDetails<'a> {
    fn new(item: &'a Item, session: &'a str, opts: DispatchOpts, agent: &'a str) -> Self {
        ConfirmationDetails {
            id: &item.id,
            title: &item.session,
            mode: DispatchMode::new(opts.inline, session),
            agent,
            autonomy: Enabled::from(opts.launch.auto.into_inner()),
            worktree: Enabled::from(opts.launch.worktree.into_inner()),
        }
    }

    fn table(&self) -> String {
        let target = self.mode.target();
        format!(
            "| id | title | mode | agent | autonomy | worktree | target |\n\
             |---|---|---|---|---|---|---|\n\
             | {} | {} | {} | {} | {} | {} | {} |",
            cell(self.id),
            cell(self.title),
            self.mode.label(),
            cell(self.agent),
            self.autonomy.label(),
            self.worktree.label(),
            cell(&target),
        )
    }
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

fn cell(value: &str) -> String {
    value.replace(['\r', '\n'], " ").replace('|', "\\|")
}
