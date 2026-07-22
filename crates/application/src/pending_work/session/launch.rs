//! Builds provider-neutral agent launches and canonical multiplexer targets.

use pwf_domain::pending_work::{ProjectPrefix, WorkItemId};

use super::{DispatchTarget, LaunchDirectives};
use crate::pending_work::list::PendingWorkItemView;

/// Autonomy directive inserted by `--auto`.
const AUTONOMY_DIRECTIVE: &str = "You MUST execute this autonomously. Do not prompt the user for questions. But if something seems critical and needs user decision, STOP execution and clarify";

pub(super) fn thread_title(item: &PendingWorkItemView) -> String {
    let mut title = String::with_capacity(item.id.len() + item.session.len() + 3);
    title.push_str(&item.id);
    title.push_str(" - ");
    title.push_str(&item.session);
    title
}

pub(super) fn launch_prompt(item: &PendingWorkItemView, directives: LaunchDirectives) -> String {
    let mut prompt = format_prompt(item, directives.autonomous);
    if directives.worktree {
        prompt.push_str("\n\n");
        prompt.push_str(&worktree_instruction(&item.id));
    }
    prompt.trim().to_string()
}

fn format_prompt(item: &PendingWorkItemView, autonomous: bool) -> String {
    let mut header = String::new();
    header.push_str("Pending-work ID: ");
    header.push_str(&item.id);
    header.push_str("\nProject: ");
    header.push_str(&item.project);
    if autonomous {
        header.push('\n');
        header.push_str(AUTONOMY_DIRECTIVE);
    }
    header.push_str("\n\ndo ");
    header.push_str(&item.id);
    header
}

fn worktree_instruction(id: &str) -> String {
    let mut instruction = String::new();
    instruction.push_str(
        "Workspace: before doing anything else, use a git-worktrees skill to create a git worktree here named `",
    );
    instruction.push_str(id);
    instruction.push_str(
        "` (the worktree name is this task's id), and do all of this task's work inside that worktree.",
    );
    instruction
}

pub(super) fn dispatch_target(task_id: &str) -> DispatchTarget {
    let Ok(work_item_id) = WorkItemId::try_new(task_id) else {
        return legacy_dispatch_target(task_id);
    };
    let canonical_id = work_item_id.as_ref();
    let prefix = canonical_id
        .split_once('-')
        .map_or("", |(prefix, _)| prefix);
    let project_prefix = ProjectPrefix::try_new(prefix)
        .expect("a validated work-item id always contains a valid project prefix");
    DispatchTarget {
        session: project_prefix.as_ref().to_ascii_lowercase(),
        tab: canonical_id.to_string(),
    }
}

fn legacy_dispatch_target(task_id: &str) -> DispatchTarget {
    DispatchTarget {
        session: task_id
            .split('-')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase(),
        tab: task_id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use pwf_domain::pending_work::WorkItemStatus;

    use super::{PendingWorkItemView, dispatch_target};
    use crate::pending_work::session::{Agent, AgentLaunch, LaunchDirectives};

    fn item() -> PendingWorkItemView {
        PendingWorkItemView {
            id: "PWF-0076".to_string(),
            project: "pwf".to_string(),
            status: WorkItemStatus::Active,
            session: "make session -w".to_string(),
            prompt: "assemble the widget".to_string(),
            repo: Some("/repo/pwf".to_string()),
            note: "assemble the widget".to_string(),
            item_file: Some("/notes/PWF-0076.md".to_string()),
            line: 1,
            format: "file".to_string(),
            launchable: true,
            needs_prompt: false,
            issues: Vec::new(),
            section: None,
            prereq: None,
            prerequisite_statuses: Vec::new(),
            tags: None,
            effort: None,
            created: Some("2026-07-15".to_string()),
        }
    }

    fn launch(worktree: bool, autonomous: bool) -> super::super::AgentLaunch {
        AgentLaunch::new(
            &item(),
            LaunchDirectives {
                worktree,
                autonomous,
            },
            Agent::Claude,
            None,
        )
    }

    #[test]
    fn plain_prompt_is_a_thin_pointer_without_the_note_body() {
        let launch = launch(false, false);

        assert_eq!(
            launch.prompt,
            "Pending-work ID: PWF-0076\nProject: pwf\n\ndo PWF-0076"
        );
        assert!(!launch.prompt.contains("assemble the widget"));
    }

    #[test]
    fn autonomous_prompt_keeps_the_exact_directive_in_the_header() {
        let launch = launch(false, true);

        assert_eq!(
            launch.prompt,
            "Pending-work ID: PWF-0076\nProject: pwf\nYou MUST execute this autonomously. Do not prompt the user for questions. But if something seems critical and needs user decision, STOP execution and clarify\n\ndo PWF-0076"
        );
    }

    #[test]
    fn worktree_prompt_keeps_the_exact_instruction() {
        let launch = launch(true, false);

        assert_eq!(
            launch.prompt,
            "Pending-work ID: PWF-0076\nProject: pwf\n\ndo PWF-0076\n\nWorkspace: before doing anything else, use a git-worktrees skill to create a git worktree here named `PWF-0076` (the worktree name is this task's id), and do all of this task's work inside that worktree."
        );
    }

    #[test]
    fn prompt_directives_compose_without_closeout_policy() {
        let launch = launch(true, true);

        assert!(launch.prompt.contains("execute this autonomously"));
        assert!(launch.prompt.contains("git-worktrees skill"));
        assert!(!launch.prompt.contains("Closeout:"));
        assert!(!launch.prompt.contains("pwf done"));
    }

    #[test]
    fn launch_carries_only_semantic_agent_values() {
        let launch = AgentLaunch::new(
            &item(),
            LaunchDirectives::default(),
            Agent::Claude,
            Some("opus".to_string()),
        );

        assert_eq!(launch.agent, Agent::Claude);
        assert_eq!(launch.task_id, "PWF-0076");
        assert_eq!(launch.title, "PWF-0076 - make session -w");
        assert_eq!(launch.repository, "/repo/pwf");
        assert_eq!(launch.model.as_deref(), Some("opus"));
    }

    #[test]
    fn canonical_target_normalizes_id_and_lowercases_the_typed_prefix() {
        let target = dispatch_target("cfg9");

        assert_eq!(target.session, "cfg");
        assert_eq!(target.tab, "CFG-0009");
    }

    #[test]
    fn legacy_inline_target_preserves_the_legacy_id_fallback() {
        let target = dispatch_target("pwf:1");

        assert_eq!(target.session, "pwf:1");
        assert_eq!(target.tab, "pwf:1");
    }
}
