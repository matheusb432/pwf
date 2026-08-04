//! Builds provider-neutral agent launches and typed multiplexer targets.

use std::error::Error;

use askama::Template;
use pwf_models::{
    session::PushedPrompt,
    task::{EffortTier, TaskId},
};
use thiserror::Error;

use super::{Agent, DispatchTarget, LaunchDirectives, ModelTierLookup, SessionEffort};
use crate::task::dto::TaskView;

/// Autonomy directive inserted by `--auto`.
const AUTONOMY_DIRECTIVE: &str = "You MUST execute this autonomously. Do not prompt the user for questions. But if something seems critical and needs user decision, STOP execution and clarify";
const SESSION_CONTEXT_OPEN: &str = "<pwf_session_context>\n";
const SESSION_CONTEXT_CLOSE: &str = "\n</pwf_session_context>";
const PWF_TASK_OPEN: &str = "<pwf_task>\n";
const PWF_TASK_CLOSE: &str = "</pwf_task>";
const WORKTREE_DIRECTIVE_PREFIX: &str = "Workspace: before doing anything else, use a git-worktrees skill to create a git worktree here named `";
const WORKTREE_DIRECTIVE_SUFFIX: &str =
    "` (the worktree name is this task's id), and do all of this task's work inside that worktree.";

fn session_context_rendered_len(
    task_id: &TaskId,
    pushed_prompt: Option<&PushedPrompt>,
    directives: LaunchDirectives,
) -> usize {
    let content_count = usize::from(pushed_prompt.is_some())
        + usize::from(directives.autonomous)
        + usize::from(directives.worktree);
    if content_count == 0 {
        return 0;
    }

    SESSION_CONTEXT_OPEN.len()
        + SESSION_CONTEXT_CLOSE.len()
        + pushed_prompt.map_or(0, |prompt| prompt.as_ref().len())
        + usize::from(directives.autonomous) * AUTONOMY_DIRECTIVE.len()
        + usize::from(directives.worktree)
            * (WORKTREE_DIRECTIVE_PREFIX.len()
                + task_id.as_ref().len()
                + WORKTREE_DIRECTIVE_SUFFIX.len())
        + (content_count - 1) * 2
}

fn write_session_context(
    output: &mut String,
    task_id: &TaskId,
    pushed_prompt: Option<&PushedPrompt>,
    directives: LaunchDirectives,
) {
    if pushed_prompt.is_none() && !directives.autonomous && !directives.worktree {
        return;
    }

    output.push_str(SESSION_CONTEXT_OPEN);
    let mut has_content = false;
    if let Some(pushed_prompt) = pushed_prompt {
        write_context_separator(output, &mut has_content);
        output.push_str(pushed_prompt.as_ref());
    }
    if directives.autonomous {
        write_context_separator(output, &mut has_content);
        output.push_str(AUTONOMY_DIRECTIVE);
    }
    if directives.worktree {
        write_context_separator(output, &mut has_content);
        output.push_str(WORKTREE_DIRECTIVE_PREFIX);
        output.push_str(task_id.as_ref());
        output.push_str(WORKTREE_DIRECTIVE_SUFFIX);
    }
    output.push_str(SESSION_CONTEXT_CLOSE);
}

fn write_context_separator(output: &mut String, has_content: &mut bool) {
    if *has_content {
        output.push_str("\n\n");
    } else {
        *has_content = true;
    }
}

#[derive(Template)]
#[template(path = "thread_title.txt")]
#[allow(
    dead_code,
    reason = "fields form the compile-time thread-title template context"
)]
struct ThreadTitleTemplate<'a> {
    task_id: &'a TaskId,
    task_id_brief: String,
    task_title: &'a str,
    project: &'a str,
    effort: SessionEffort,
    autonomous: bool,
    worktree: bool,
    agent: &'static str,
}

pub(super) fn thread_title(
    task: &TaskView,
    task_id: &TaskId,
    directives: LaunchDirectives,
    agent: Agent,
    effort: SessionEffort,
) -> Result<String, askama::Error> {
    ThreadTitleTemplate {
        task_id,
        task_id_brief: task_id_brief(task_id),
        task_title: &task.session,
        project: &task.project,
        effort,
        autonomous: directives.autonomous,
        worktree: directives.worktree,
        agent: match agent {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
        },
    }
    .render()
}

fn task_id_brief(task_id: &TaskId) -> String {
    let project_id = task_id.project_id();
    let digits = task_id
        .as_ref()
        .strip_prefix(project_id.as_ref())
        .and_then(|suffix| suffix.strip_prefix('-'))
        .unwrap_or_default();
    let digits = digits.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };

    format!("{}{digits}", project_id.as_ref().to_ascii_lowercase())
}

pub(super) fn launch_prompt(
    task_content: &str,
    task_id: &TaskId,
    pushed_prompt: Option<&PushedPrompt>,
    directives: LaunchDirectives,
) -> String {
    let session_context_len = session_context_rendered_len(task_id, pushed_prompt, directives);
    let separator_len = usize::from(session_context_len > 0) * 2;
    let task_len = PWF_TASK_OPEN.len()
        + task_content.len()
        + usize::from(!task_content.ends_with('\n'))
        + PWF_TASK_CLOSE.len();
    let prompt_len = session_context_len + separator_len + task_len;
    let mut prompt = String::with_capacity(prompt_len);
    write_session_context(&mut prompt, task_id, pushed_prompt, directives);
    if session_context_len > 0 {
        prompt.push_str("\n\n");
    }
    prompt.push_str(PWF_TASK_OPEN);
    prompt.push_str(task_content);
    if !task_content.ends_with('\n') {
        prompt.push('\n');
    }
    prompt.push_str(PWF_TASK_CLOSE);
    debug_assert_eq!(prompt.len(), prompt_len);
    prompt
}

pub(super) fn dispatch_target(task_id: &TaskId) -> DispatchTarget {
    DispatchTarget {
        task_id: task_id.clone(),
    }
}

#[derive(Debug, Error)]
pub(super) enum ModelSelectionError {
    #[error(
        "task {task_id} has an invalid effort value '{value}' (expected low, medium, high, or highest)."
    )]
    InvalidEffort { task_id: TaskId, value: String },
    #[error("{0}")]
    Catalog(#[source] Box<dyn Error + Send + Sync>),
    #[error("tier {tier} has no [tiers.{tier}] entry in {catalog}")]
    MissingTier { tier: EffortTier, catalog: String },
    #[error("tier {tier} in {catalog} has no claude_model set")]
    MissingClaudeModel { tier: EffortTier, catalog: String },
}

pub(super) fn resolve_model<E>(
    agent: Agent,
    task_id: &TaskId,
    effort: Option<&str>,
    model_tier: impl FnOnce(EffortTier) -> Result<ModelTierLookup, E>,
) -> Result<Option<String>, ModelSelectionError>
where
    E: Error + Send + Sync + 'static,
{
    if agent == Agent::Codex {
        return Ok(None);
    }
    let Some(raw_effort) = effort else {
        return Ok(None);
    };
    let tier = parse_effort(raw_effort).ok_or_else(|| ModelSelectionError::InvalidEffort {
        task_id: task_id.clone(),
        value: raw_effort.to_string(),
    })?;
    let ModelTierLookup {
        catalog,
        tier: entry,
    } = model_tier(tier).map_err(|error| ModelSelectionError::Catalog(Box::new(error)))?;
    let Some(entry) = entry else {
        return Err(ModelSelectionError::MissingTier { tier, catalog });
    };
    let model = entry
        .claude_model
        .ok_or(ModelSelectionError::MissingClaudeModel { tier, catalog })?;
    Ok(if model.is_empty() { None } else { Some(model) })
}

fn parse_effort(raw: &str) -> Option<EffortTier> {
    raw.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use pwf_models::{session::PushedPrompt, task::TaskId};

    use super::{dispatch_target, launch_prompt};
    use crate::task::session::LaunchDirectives;

    #[test]
    fn launch_prompt_wraps_the_task_without_rendering_an_empty_session_context() {
        let task_id = TaskId::try_new("PWF-0076").unwrap();

        assert_eq!(
            launch_prompt("task content", &task_id, None, LaunchDirectives::default()),
            "<pwf_task>\ntask content\n</pwf_task>"
        );
    }

    #[test]
    fn launch_prompt_orders_all_session_context_content_before_the_task() {
        let task_id = TaskId::try_new("PWF-0076").unwrap();
        let pushed_prompt = PushedPrompt::try_new("extra context").unwrap();

        assert_eq!(
            launch_prompt(
                "task content",
                &task_id,
                Some(&pushed_prompt),
                LaunchDirectives {
                    autonomous: true,
                    worktree: true,
                },
            ),
            concat!(
                "<pwf_session_context>\n",
                "extra context\n\n",
                "You MUST execute this autonomously. Do not prompt the user for questions. But if something seems critical and needs user decision, STOP execution and clarify\n\n",
                "Workspace: before doing anything else, use a git-worktrees skill to create a git worktree here named `PWF-0076` (the worktree name is this task's id), and do all of this task's work inside that worktree.\n",
                "</pwf_session_context>\n\n",
                "<pwf_task>\n",
                "task content\n",
                "</pwf_task>",
            )
        );
    }

    #[test]
    fn task_wrapper_preserves_an_existing_trailing_newline_without_adding_a_blank_line() {
        let task_id = TaskId::try_new("PWF-0076").unwrap();

        assert_eq!(
            launch_prompt(
                "task content\n",
                &task_id,
                None,
                LaunchDirectives::default()
            ),
            "<pwf_task>\ntask content\n</pwf_task>"
        );
    }

    #[test]
    fn target_uses_the_typed_id_and_lowercases_its_project_id() {
        let task_id = "cfg9".parse::<TaskId>().unwrap();
        let target = dispatch_target(&task_id);

        assert_eq!(target.task_id, task_id);
        assert_eq!(target.session_name(), "cfg");
    }
}

#[cfg(test)]
mod model_selection_tests {
    use std::{assert_matches, error::Error, fmt};

    use pwf_models::task::{EffortTier, TaskId};

    use super::{ModelSelectionError, parse_effort, resolve_model};
    use crate::task::session::{Agent, ModelTier, ModelTierLookup};

    const CATALOG_PATH: &str = "/config/model-tiers.toml";

    fn task_id() -> TaskId {
        TaskId::try_new("PWF-0001").unwrap()
    }

    #[derive(Debug, Clone)]
    struct CatalogError(&'static str);

    impl fmt::Display for CatalogError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.0)
        }
    }

    impl Error for CatalogError {}

    fn catalog(claude_model: Option<&str>) -> ModelTierLookup {
        ModelTierLookup {
            catalog: CATALOG_PATH.to_string(),
            tier: Some(ModelTier {
                claude_model: claude_model.map(str::to_string),
            }),
        }
    }

    #[test]
    fn effort_text_decodes_only_plain_english_names() {
        for (raw, expected) in [
            ("low", EffortTier::Low),
            ("medium", EffortTier::Medium),
            ("high", EffortTier::High),
            ("highest", EffortTier::Highest),
        ] {
            assert_eq!(parse_effort(raw), Some(expected));
        }
        for raw in ["1", "4", "abc", ""] {
            assert!(parse_effort(raw).is_none());
        }
    }

    #[test]
    fn codex_ignores_effort_and_the_catalog() {
        let model = resolve_model(Agent::Codex, &task_id(), Some("nine"), |_| {
            Err(CatalogError("catalog unavailable"))
        })
        .unwrap();

        assert_eq!(model, None);
    }

    #[test]
    fn claude_without_effort_does_not_read_the_catalog() {
        let model = resolve_model(Agent::Claude, &task_id(), None, |_| {
            Err(CatalogError("catalog unavailable"))
        })
        .unwrap();

        assert_eq!(model, None);
    }

    #[test]
    fn malformed_effort_is_an_application_error() {
        let error = resolve_model(Agent::Claude, &task_id(), Some("nine"), |_| {
            Ok::<_, CatalogError>(catalog(Some("sonnet")))
        })
        .unwrap_err();

        assert_matches!(
            error,
            ModelSelectionError::InvalidEffort {
                task_id: ref actual_task_id,
                ref value,
            } if actual_task_id == &task_id() && value == "nine"
        );
    }

    #[test]
    fn configured_claude_model_is_selected() {
        let model = resolve_model(Agent::Claude, &task_id(), Some("high"), |_| {
            Ok::<_, CatalogError>(catalog(Some("sonnet")))
        })
        .unwrap();

        assert_eq!(model.as_deref(), Some("sonnet"));
    }

    #[test]
    fn empty_claude_model_is_the_no_override_sentinel() {
        let model = resolve_model(Agent::Claude, &task_id(), Some("medium"), |_| {
            Ok::<_, CatalogError>(catalog(Some("")))
        })
        .unwrap();

        assert_eq!(model, None);
    }

    #[test]
    fn catalog_read_error_retains_its_source() {
        let error = resolve_model(Agent::Claude, &task_id(), Some("low"), |_| {
            Err(CatalogError("catalog unavailable"))
        })
        .unwrap_err();

        assert_eq!(error.source().unwrap().to_string(), "catalog unavailable");
    }

    #[test]
    fn missing_tier_is_an_application_error() {
        let error = resolve_model(Agent::Claude, &task_id(), Some("highest"), |_| {
            Ok::<_, CatalogError>(ModelTierLookup {
                catalog: CATALOG_PATH.to_string(),
                tier: None,
            })
        })
        .unwrap_err();

        assert_matches!(
            &error,
            ModelSelectionError::MissingTier {
                tier: EffortTier::Highest,
                ..
            }
        );
        assert_eq!(
            error.to_string(),
            format!("tier highest has no [tiers.highest] entry in {CATALOG_PATH}")
        );
    }

    #[test]
    fn missing_claude_model_is_an_application_error() {
        let error = resolve_model(Agent::Claude, &task_id(), Some("highest"), |_| {
            Ok::<_, CatalogError>(catalog(None))
        })
        .unwrap_err();

        assert_matches!(
            &error,
            ModelSelectionError::MissingClaudeModel {
                tier: EffortTier::Highest,
                ..
            }
        );
        assert_eq!(
            error.to_string(),
            format!("tier highest in {CATALOG_PATH} has no claude_model set")
        );
    }
}
