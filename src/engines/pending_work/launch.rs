// Launch prompt construction shared by session and verify.

use nutype::nutype;

use super::model::Item;

/// Exact autonomy directive `--auto` writes into the launch prompt header.
const AUTONOMY_DIRECTIVE: &str = "You MUST execute this autonomously. Do not prompt the user for questions. But if something seems critical and needs user decision, STOP execution and clarify";

/// Whether `pwf session` tells the dispatched agent to isolate its work in a git
/// worktree (`-w`/`--worktree`). A newtype, not a bare `bool`, so it can't be
/// transposed with the other dispatch-policy flags and reads for what it means.
#[nutype(
    default = false,
    derive(Debug, Clone, Copy, PartialEq, Eq, Default, From)
)]
pub(crate) struct Worktree(bool);

/// Whether `pwf session --auto` tells the dispatched agent to run autonomously
/// without prompting the user. A newtype for the same reason as [`Worktree`].
#[nutype(
    default = false,
    derive(Debug, Clone, Copy, PartialEq, Eq, Default, From)
)]
pub(crate) struct Auto(bool);

/// Which optional instruction blocks ride in the dispatched agent's launch prompt.
/// Bundled so the launcher seam takes one named value — no adjacent-bool
/// transposition as prompt-augmentation flags accrete.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LaunchPolicy {
    pub worktree: Worktree,
    pub auto: Auto,
}

pub(super) fn new_launch_prompt(item: &Item, policy: LaunchPolicy) -> String {
    let mut out = format_prompt(item, policy.auto);
    if policy.worktree.into_inner() {
        out.push_str("\n\n");
        out.push_str(&worktree_instruction(&item.id));
    }
    out.trim().to_string()
}

/// The prompt header (`Pending-work ID:` / `Project:`) and a thin pointer to start
/// the task, rather than the note's full body: the dispatched agent's first step
/// (the pw-workflow skill) already resolves the item and reads the body in full,
/// so inlining it here would just be redundant. With `auto` set, the autonomy
/// directive rides as the final header line, before the pointer.
fn format_prompt(item: &Item, auto: Auto) -> String {
    let mut header = format!("Pending-work ID: {}\nProject: {}", item.id, item.project);
    if auto.into_inner() {
        header.push('\n');
        header.push_str(AUTONOMY_DIRECTIVE);
    }
    format!("{header}\n\ndo {}", item.id)
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
            prompt: "assemble the widget".to_string(),
            ..Item::default_for_test("PWF-0076", "make session -w")
        }
    }

    fn policy(worktree: bool, auto: bool) -> LaunchPolicy {
        LaunchPolicy {
            worktree: Worktree::from(worktree),
            auto: Auto::from(auto),
        }
    }

    #[test]
    fn worktree_false_leaves_prompt_unaugmented() {
        let out = new_launch_prompt(&item(), LaunchPolicy::default());
        assert!(out.contains("do PWF-0076"), "carries the thin pointer");
        assert!(
            !out.contains("assemble the widget"),
            "note body must not be inlined: {out}"
        );
        assert!(!out.to_lowercase().contains("worktree"));
    }

    #[test]
    fn worktree_true_appends_named_worktree_instruction() {
        let out = new_launch_prompt(&item(), policy(true, false));
        assert!(out.contains("do PWF-0076"), "keeps the thin pointer");
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
    fn prompt_never_carries_a_closeout_line() {
        let plain = new_launch_prompt(&item(), LaunchPolicy::default());
        let augmented = new_launch_prompt(&item(), policy(true, true));
        for out in [&plain, &augmented] {
            assert!(
                !out.contains("Closeout:"),
                "no closeout instruction rides in the launch prompt: {out}"
            );
            assert!(
                !out.contains("pwf done"),
                "no closeout command rides in the launch prompt: {out}"
            );
        }
    }

    #[test]
    fn auto_false_leaves_prompt_without_directive() {
        let out = new_launch_prompt(&item(), LaunchPolicy::default());
        assert!(!out.contains("execute this autonomously"));
    }

    #[test]
    fn auto_true_writes_exact_directive_into_header() {
        let out = new_launch_prompt(&item(), policy(false, true));
        assert!(out.contains("do PWF-0076"), "keeps the thin pointer");
        assert!(
            out.contains(AUTONOMY_DIRECTIVE),
            "carries the exact autonomy directive: {out}"
        );
        // The directive rides in the header, ahead of the pointer.
        let dir = out.find(AUTONOMY_DIRECTIVE).expect("directive present");
        let pointer = out.find("do PWF-0076").expect("pointer present");
        assert!(
            dir < pointer,
            "directive is a header line, before the pointer"
        );
    }

    #[test]
    fn auto_composes_with_worktree() {
        let out = new_launch_prompt(&item(), policy(true, true));
        assert!(
            out.contains(AUTONOMY_DIRECTIVE),
            "autonomy directive present"
        );
        assert!(out.contains("git-worktrees skill"), "worktree step present");
    }
}
