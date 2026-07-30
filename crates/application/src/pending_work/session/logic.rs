//! Builds provider-neutral agent launches and canonical multiplexer targets.

use std::error::Error;

use askama::Template;
use pwf_models::pending_work::{EffortTier, ProjectPrefix};
use thiserror::Error;

use super::{Agent, AgentLaunch, DispatchTarget, LaunchDirectives, ModelTierLookup, SessionEffort};
use crate::{
    AgentModelTierCatalogClient, AppRecordStore, NoteMarkdownClient, PendingWorkRecord,
    pending_work::{
        ProjectRegistry,
        dto::PendingWorkItemView,
        identifier,
        show_pending_work_item::{
            self, ShowOutput, ShowPendingWorkError, ShowPendingWorkItem, ShowPendingWorkItemOk,
        },
    },
};

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

pub(super) fn agent_launch(
    item: &PendingWorkItemView,
    task_content: &str,
    directives: LaunchDirectives,
    agent: Agent,
    model: Option<String>,
    effort: SessionEffort,
) -> AgentLaunch {
    AgentLaunch {
        agent,
        task_id: item.id.clone(),
        title: thread_title(item, directives, agent, effort),
        repository: item.repo.clone().unwrap_or_default(),
        prompt: launch_prompt(task_content, &item.id, directives),
        model,
        effort,
    }
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

#[derive(Debug, Error)]
pub(super) enum ModelSelectionError {
    #[error(
        "item {task_id} has an invalid effort value '{value}' (expected low, medium, high, or highest)."
    )]
    InvalidEffort { task_id: String, value: String },
    #[error("{0}")]
    Catalog(#[source] Box<dyn Error + Send + Sync>),
    #[error("tier {tier} has no [tiers.{tier}] entry in {catalog}")]
    MissingTier { tier: EffortTier, catalog: String },
    #[error("tier {tier} in {catalog} has no claude_model set")]
    MissingClaudeModel { tier: EffortTier, catalog: String },
}

pub(super) fn resolve_model<C>(
    catalog: &C,
    agent: Agent,
    task_id: &str,
    effort: Option<&str>,
) -> Result<Option<String>, ModelSelectionError>
where
    C: AgentModelTierCatalogClient,
{
    if agent == Agent::Codex {
        return Ok(None);
    }
    let Some(raw_effort) = effort else {
        return Ok(None);
    };
    let tier = parse_effort(raw_effort).ok_or_else(|| ModelSelectionError::InvalidEffort {
        task_id: task_id.to_string(),
        value: raw_effort.to_string(),
    })?;
    let ModelTierLookup {
        catalog,
        tier: entry,
    } = catalog
        .tier(tier)
        .map_err(|error| ModelSelectionError::Catalog(Box::new(error)))?;
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

pub(super) fn load_task_content(
    id: &str,
    store: &impl AppRecordStore<PendingWorkRecord>,
    projects: &ProjectRegistry,
    markdown_client: &impl NoteMarkdownClient,
) -> Result<String, ShowPendingWorkError> {
    let output = show_pending_work_item::execute(
        &ShowPendingWorkItem {
            id: id.to_string(),
            output: ShowOutput::Markdown,
        },
        store,
        projects,
        markdown_client,
    )?;
    let ShowPendingWorkItemOk::Markdown(markdown) = output else {
        unreachable!("Markdown request returned a different representation")
    };
    Ok(markdown)
}

#[cfg(test)]
mod tests {
    use pwf_models::pending_work::WorkItemStatus;

    use super::{PendingWorkItemView, agent_launch, dispatch_target};
    use crate::pending_work::session::{Agent, LaunchDirectives, SessionEffort};

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
        let launch = agent_launch(
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

#[cfg(test)]
mod model_selection_tests {
    use std::{assert_matches, error::Error, fmt};

    use pwf_models::pending_work::EffortTier;

    use super::{ModelSelectionError, parse_effort, resolve_model};
    use crate::{
        AgentModelTierCatalogClient,
        pending_work::session::{Agent, ModelTier, ModelTierLookup},
    };

    const CATALOG_PATH: &str = "/config/model-tiers.toml";

    #[derive(Debug, Clone)]
    struct CatalogError(&'static str);

    impl fmt::Display for CatalogError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.0)
        }
    }

    impl Error for CatalogError {}

    #[derive(Clone)]
    struct Catalog {
        result: Result<ModelTierLookup, CatalogError>,
    }

    impl AgentModelTierCatalogClient for Catalog {
        type Error = CatalogError;

        fn tier(&self, _effort: EffortTier) -> Result<ModelTierLookup, Self::Error> {
            self.result.clone()
        }
    }

    fn catalog(claude_model: Option<&str>) -> Catalog {
        Catalog {
            result: Ok(ModelTierLookup {
                catalog: CATALOG_PATH.to_string(),
                tier: Some(ModelTier {
                    claude_model: claude_model.map(str::to_string),
                }),
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
        let unavailable_catalog = Catalog {
            result: Err(CatalogError("catalog unavailable")),
        };

        let model =
            resolve_model(&unavailable_catalog, Agent::Codex, "PWF-0001", Some("nine")).unwrap();

        assert_eq!(model, None);
    }

    #[test]
    fn claude_without_effort_does_not_read_the_catalog() {
        let unavailable_catalog = Catalog {
            result: Err(CatalogError("catalog unavailable")),
        };

        let model = resolve_model(&unavailable_catalog, Agent::Claude, "PWF-0001", None).unwrap();

        assert_eq!(model, None);
    }

    #[test]
    fn malformed_effort_is_an_application_error() {
        let error = resolve_model(
            &catalog(Some("sonnet")),
            Agent::Claude,
            "PWF-0001",
            Some("nine"),
        )
        .unwrap_err();

        assert_matches!(
            error,
            ModelSelectionError::InvalidEffort { ref task_id, ref value }
                if task_id == "PWF-0001" && value == "nine"
        );
    }

    #[test]
    fn configured_claude_model_is_selected() {
        let model = resolve_model(
            &catalog(Some("sonnet")),
            Agent::Claude,
            "PWF-0001",
            Some("high"),
        )
        .unwrap();

        assert_eq!(model.as_deref(), Some("sonnet"));
    }

    #[test]
    fn empty_claude_model_is_the_no_override_sentinel() {
        let model = resolve_model(
            &catalog(Some("")),
            Agent::Claude,
            "PWF-0001",
            Some("medium"),
        )
        .unwrap();

        assert_eq!(model, None);
    }

    #[test]
    fn catalog_read_error_retains_its_source() {
        let unavailable_catalog = Catalog {
            result: Err(CatalogError("catalog unavailable")),
        };

        let error = resolve_model(&unavailable_catalog, Agent::Claude, "PWF-0001", Some("low"))
            .unwrap_err();

        assert_eq!(error.source().unwrap().to_string(), "catalog unavailable");
    }

    #[test]
    fn missing_tier_is_an_application_error() {
        let missing = Catalog {
            result: Ok(ModelTierLookup {
                catalog: CATALOG_PATH.to_string(),
                tier: None,
            }),
        };

        let error =
            resolve_model(&missing, Agent::Claude, "PWF-0001", Some("highest")).unwrap_err();

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
        let error =
            resolve_model(&catalog(None), Agent::Claude, "PWF-0001", Some("highest")).unwrap_err();

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
