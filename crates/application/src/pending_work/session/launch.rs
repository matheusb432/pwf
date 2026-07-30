//! Builds provider-neutral agent launches and canonical multiplexer targets.

use askama::Template;
use pwf_models::pending_work::ProjectPrefix;

use super::{Agent, DispatchTarget, LaunchDirectives, SessionEffort};
use crate::pending_work::{get_pending_work::PendingWorkItemView, identifier};

/// Autonomy directive inserted by `--auto`.
const AUTONOMY_DIRECTIVE: &str = "You MUST execute this autonomously. Do not prompt the user for questions. But if something seems critical and needs user decision, STOP execution and clarify";

#[derive(Template)]
#[template(path = "thread_title.txt")]
#[allow(
    dead_code,
    reason = "fields form the compile-time thread-title template context"
)]
struct ThreadTitleTemplate<'a> {
    task_id: &'a str,
    task_id_brief: String,
    task_title: &'a str,
    project: &'a str,
    effort: SessionEffort,
    autonomous: bool,
    worktree: bool,
    agent: &'static str,
}

pub(super) fn thread_title(
    item: &PendingWorkItemView,
    directives: LaunchDirectives,
    agent: Agent,
    effort: SessionEffort,
) -> String {
    ThreadTitleTemplate {
        task_id: &item.id,
        task_id_brief: task_id_brief(&item.id),
        task_title: &item.session,
        project: &item.project,
        effort,
        autonomous: directives.autonomous,
        worktree: directives.worktree,
        agent: match agent {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
        },
    }
    .render()
    .expect("the compile-time-checked thread-title template renders to a String")
}

fn task_id_brief(task_id: &str) -> String {
    let Some(work_item_id) = identifier::parse(task_id) else {
        return task_id.to_string();
    };
    let (prefix, digits) = work_item_id
        .as_ref()
        .split_once('-')
        .expect("a validated work-item id contains a separator");
    let number = digits
        .parse::<u16>()
        .expect("a validated work-item id contains numeric digits");

    format!("{}{number}", prefix.to_ascii_lowercase())
}

pub(super) fn launch_prompt(
    task_content: &str,
    task_id: &str,
    directives: LaunchDirectives,
) -> String {
    let mut prompt = String::new();
    if directives.autonomous {
        prompt.push_str(AUTONOMY_DIRECTIVE);
        prompt.push_str("\n\n");
    }
    prompt.push_str(task_content);
    if directives.worktree {
        if !prompt.ends_with('\n') {
            prompt.push('\n');
        }
        prompt.push('\n');
        prompt.push_str(&worktree_instruction(task_id));
    }
    prompt
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
    let Some(work_item_id) = identifier::parse(task_id) else {
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
        window: canonical_id.to_string(),
    }
}

fn legacy_dispatch_target(task_id: &str) -> DispatchTarget {
    DispatchTarget {
        session: task_id
            .split('-')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase(),
        window: task_id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use pwf_models::pending_work::WorkItemStatus;

    use super::{PendingWorkItemView, dispatch_target};
    use crate::pending_work::session::{Agent, AgentLaunch, LaunchDirectives, SessionEffort};

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

    #[test]
    fn launch_carries_only_semantic_agent_values() {
        let launch = AgentLaunch::new(
            &item(),
            "task content",
            LaunchDirectives::default(),
            Agent::Claude,
            Some("opus".to_string()),
            SessionEffort::XHigh,
        );

        assert_eq!(launch.agent, Agent::Claude);
        assert_eq!(launch.task_id, "PWF-0076");
        assert_eq!(launch.title, "pwf76 :: make session -w");
        assert_eq!(launch.repository, "/repo/pwf");
        assert_eq!(launch.model.as_deref(), Some("opus"));
        assert_eq!(launch.effort, SessionEffort::XHigh);
    }

    #[test]
    fn canonical_target_normalizes_id_and_lowercases_the_typed_prefix() {
        let target = dispatch_target("cfg9");

        assert_eq!(target.session, "cfg");
        assert_eq!(target.window, "CFG-0009");
    }

    #[test]
    fn legacy_inline_target_preserves_the_legacy_id_fallback() {
        let target = dispatch_target("pwf:1");

        assert_eq!(target.session, "pwf:1");
        assert_eq!(target.window, "pwf:1");
    }
}
