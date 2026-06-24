// Launch prompt construction shared by session and verify.

use nutype::nutype;

use super::model::Item;

/// Whether `pwf session` tells the dispatched agent to isolate its work in a git
/// worktree (`-w`/`--worktree`). A newtype, not a bare `bool`, so it can't be
/// transposed with the other dispatch-policy flags and reads for what it means.
#[nutype(
    default = false,
    derive(Debug, Clone, Copy, PartialEq, Eq, Default, From)
)]
pub(crate) struct Worktree(bool);

fn is_adhoc(item: &Item) -> bool {
    item.id.starts_with("adhoc:")
}

fn report_closeout_command(id: &str) -> String {
    format!("pwf check --id {id} --report \"<brief result>\"")
}

fn report_closeout_text(item: &Item) -> Option<String> {
    if is_adhoc(item) {
        return None;
    }
    Some(format!(
        "Closeout: if no handoff or plan is the source of truth for this item, check it done with a one-line report:\n{}",
        report_closeout_command(&item.id)
    ))
}

pub(super) fn new_launch_prompt(item: &Item, worktree: Worktree) -> String {
    let mut out = format_prompt(item);
    if worktree.into_inner() {
        out.push_str("\n\n");
        out.push_str(&worktree_instruction(&item.id));
    }
    if let Some(closeout) = report_closeout_text(item) {
        out.push_str("\n\n");
        out.push_str(&closeout);
    }
    out.trim().to_string()
}

fn format_prompt(item: &Item) -> String {
    format!(
        "Pending-work ID: {}\nProject: {}\n\n{}",
        item.id, item.project, item.prompt
    )
}

/// Prepended workspace step for `pwf session -w`: instruct the agent to isolate
/// its work in a git worktree named after the item id before touching anything.
fn worktree_instruction(id: &str) -> String {
    format!(
        "Workspace: before doing anything else, use a git-worktrees skill to create a git worktree here named `{id}` (the worktree name is this task's id), and do all of this task's work inside that worktree."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::pending_work::model::Item;

    fn item() -> Item {
        Item {
            prompt: "do the thing".to_string(),
            ..Item::default_for_test("PWF-0076", "make session -w")
        }
    }

    #[test]
    fn worktree_false_leaves_prompt_unaugmented() {
        let out = new_launch_prompt(&item(), Worktree::from(false));
        assert!(out.contains("do the thing"));
        assert!(!out.to_lowercase().contains("worktree"));
    }

    #[test]
    fn worktree_true_appends_named_worktree_instruction() {
        let out = new_launch_prompt(&item(), Worktree::from(true));
        assert!(out.contains("do the thing"), "keeps the task prompt");
        assert!(
            out.contains("git-worktrees skill"),
            "points at the worktree skill"
        );
        // The worktree is named after the item id.
        assert!(
            out.contains("named `PWF-0076`"),
            "names the worktree after the id: {out}"
        );
    }

    #[test]
    fn worktree_instruction_precedes_closeout() {
        let out = new_launch_prompt(&item(), Worktree::from(true));
        let wt = out.find("Workspace:").expect("worktree line present");
        let co = out.find("Closeout:").expect("closeout line present");
        assert!(wt < co, "worktree setup comes before the closeout report");
    }
}
