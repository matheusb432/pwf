//! Plans a task session before host validation or dispatch.

use std::{
    error::Error,
    path::{Path, PathBuf},
};

use askama::Template;
use pwf_models::{
    project::{ProjectId, ProjectSourceValue},
    session::{AgentModel, PushedPrompt},
    task::{EffortTier, TaskId},
};
use pwf_wire::task::{
    TaskView,
    session::{
        AgentLaunch, AgentProbe, DispatchConfirmation, DispatchTarget, ModelTierLookup, SessionPlan,
    },
};
use thiserror::Error;

use super::{Agent, DispatchMode, LaunchDirectives, SessionEffort};
use crate::{
    ports::{
        agent::AgentClient,
        project_directory::ProjectDirectoryClient,
        project_note::ProjectNoteStore,
        session::{AgentCommand, SessionClient, SessionStart, SessionWindow},
        task_record::{Materialization, TaskRecord, TaskStore},
    },
    project::resolve_runtime_path::{self, ResolveRuntimePath},
    task::find_active_task::{self, FindActiveTask, FindActiveTaskError},
};

/// Requests one provider-neutral session plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSession {
    pub task_id: TaskId,
    pub intent: PlanSessionIntent,
    pub pushed_prompt: Option<PushedPrompt>,
    pub mode: DispatchMode,
    pub directives: LaunchDirectives,
    pub agent: Agent,
    pub model_override: AgentModel,
    pub effort: SessionEffort,
}

/// Selects whether a plan is prepared for dispatch or rendered without effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanSessionIntent {
    Dispatch,
    DryRun,
}

pub enum PlanSessionOk {
    Dispatch(PreparedSessionDispatch),
    DryRun(DryRunSession),
}

/// Contains a validated dry-run plan and its exact process argv.
pub struct DryRunSession {
    pub plan: SessionPlan,
    pub argv: Vec<String>,
    pub probe: AgentProbe,
}

/// Contains a validated session dispatch ready for confirmation.
pub struct PreparedSessionDispatch {
    pub plan: SessionPlan,
    pub confirmation: DispatchConfirmation,
    pub probe: AgentProbe,
}

#[derive(Debug, Error)]
pub enum PlanSessionError {
    #[error(transparent)]
    Find(#[from] FindActiveTaskError),
    #[error("{0}")]
    ReadTaskMarkdown(#[source] Box<dyn Error + Send + Sync>),
    #[error("Task '{id}' is not launchable: {}", issues.join("; "))]
    NotLaunchable { id: TaskId, issues: Vec<String> },
    #[error("Project path for '{project_id}' does not exist: {path}")]
    ProjectPathMissing { project_id: ProjectId, path: String },
    #[error("Session multiplexer is unavailable; cannot dispatch a pwf session.")]
    MultiplexerNotFound,
    #[error("Checking multiplexer session '{session}' failed: {message}")]
    MultiplexerSessionCheck { session: String, message: String },
    #[error("Multiplexer session '{session}' does not exist")]
    MultiplexerSessionMissing {
        session: String,
        start_command_argv: Vec<String>,
    },
    #[error("{0}")]
    ModelTier(#[source] Box<dyn Error + Send + Sync>),
    #[error("Failed to render session title: {0}")]
    RenderThreadTitle(#[source] askama::Error),
    #[error("Invalid path for project '{project_id}': {reason}")]
    InvalidProjectPath {
        project_id: ProjectId,
        reason: String,
    },
    #[error("Agent command is empty.")]
    EmptyAgentCommand,
}

/// Plans one active task without performing host I/O or persistence.
///
/// # Errors
///
/// Returns [`PlanSessionError`] for lookup, launch validation, or model selection failures.
#[cqrsy::command]
pub async fn execute(
    command: &PlanSession,
    store: &(impl TaskStore + ProjectNoteStore),
    pool: &sqlx::SqlitePool,
    home: &PathBuf,
    agent_client: &impl AgentClient,
    project_directory: &impl ProjectDirectoryClient,
    session_client: &impl SessionClient,
) -> Result<PlanSessionOk, PlanSessionError> {
    let probe = agent_client.probe(command.agent);
    let found = find_active_task::execute(
        &FindActiveTask {
            id: command.task_id.clone(),
        },
        store,
        pool,
    )
    .await?;
    let task = found.task;
    if !task.launchable {
        return Err(PlanSessionError::NotLaunchable {
            id: command.task_id.clone(),
            issues: task.issues,
        });
    }
    let project_id = found.project.id.clone();
    let project_path = resolve_project_path(found.project.source.value(), &project_id, home)?;
    let task_content = load_task_content(&found.record, store)?;

    let model: AgentModel = match command.model_override.clone().into_inner() {
        Some(model) => Some(model),
        None => resolve_model(
            command.agent,
            &command.task_id,
            task.effort.as_deref(),
            |effort| agent_client.model_tier(effort),
        )
        .map_err(|error| PlanSessionError::ModelTier(Box::new(error)))?,
    }
    .into();
    let target = dispatch_target(&command.task_id);
    let plan = SessionPlan {
        launch: AgentLaunch {
            agent: command.agent,
            task_id: command.task_id.clone(),
            title: thread_title(
                &task,
                &command.task_id,
                command.directives,
                command.agent,
                command.effort,
            )
            .map_err(PlanSessionError::RenderThreadTitle)?,
            project_path: project_path.clone(),
            prompt: launch_prompt(
                &task_content,
                &command.task_id,
                command.pushed_prompt.as_ref(),
                command.directives,
            ),
            model: model.clone().into_inner(),
            effort: command.effort,
        },
        mode: command.mode,
        target: target.clone(),
    };
    validate_project_path(project_directory, &project_id, &plan.launch.project_path)?;
    validate_multiplexer(command, &plan, session_client)?;
    let confirmation = DispatchConfirmation {
        task_id: command.task_id.clone(),
        title: task.session,
        created: task.created,
        mode: command.mode,
        agent: command.agent,
        directives: command.directives,
        has_pushed_prompt: command.pushed_prompt.is_some(),
        model: model.display_or_default(),
        effort: command.effort,
        target,
    };

    match command.intent {
        PlanSessionIntent::Dispatch => Ok(PlanSessionOk::Dispatch(PreparedSessionDispatch {
            plan,
            confirmation,
            probe,
        })),
        PlanSessionIntent::DryRun => {
            let provider_argv = agent_client.preview(&plan.launch);
            let argv = preview_dispatch_argv(provider_argv, &plan, session_client)?;
            Ok(PlanSessionOk::DryRun(DryRunSession { plan, argv, probe }))
        }
    }
}

fn preview_dispatch_argv(
    provider_argv: Vec<String>,
    plan: &SessionPlan,
    session_client: &impl SessionClient,
) -> Result<Vec<String>, PlanSessionError> {
    match plan.mode {
        DispatchMode::Inline => {
            AgentCommand::try_new(&provider_argv)
                .map_err(|_| PlanSessionError::EmptyAgentCommand)?;
            Ok(provider_argv)
        }
        DispatchMode::Multiplexer => {
            let session_name = plan.target.session_name();
            let agent_command = AgentCommand::try_new(&provider_argv)
                .map_err(|_| PlanSessionError::EmptyAgentCommand)?;
            Ok(session_client.preview_window(&SessionWindow {
                session_name: &session_name,
                working_directory: &plan.launch.project_path,
                window_name: plan.target.task_id.as_ref(),
                agent_command,
            }))
        }
    }
}

fn load_task_content(
    record: &TaskRecord,
    store: &impl ProjectNoteStore,
) -> Result<String, PlanSessionError> {
    if matches!(record.materialization, Materialization::MissingNote { .. }) {
        return store
            .read_note_markdown(&record.locator)
            .map_err(|error| PlanSessionError::ReadTaskMarkdown(Box::new(error)));
    }
    Ok(record.source.clone())
}

fn resolve_project_path(
    source_value: &ProjectSourceValue,
    project_id: &ProjectId,
    home: &Path,
) -> Result<String, PlanSessionError> {
    let resolved = resolve_runtime_path::execute(&ResolveRuntimePath {
        path: source_value.as_ref().to_string(),
        home: home.to_path_buf(),
    })
    .map_err(|error| PlanSessionError::InvalidProjectPath {
        project_id: project_id.clone(),
        reason: error.to_string(),
    })?;
    Ok(resolved.path().to_string_lossy().into_owned())
}

fn validate_project_path(
    project_directory: &impl ProjectDirectoryClient,
    project_id: &ProjectId,
    project_path: &str,
) -> Result<(), PlanSessionError> {
    if project_directory.is_directory(project_path) {
        return Ok(());
    }
    Err(PlanSessionError::ProjectPathMissing {
        project_id: project_id.clone(),
        path: project_path.to_string(),
    })
}

fn validate_multiplexer(
    command: &PlanSession,
    plan: &SessionPlan,
    session_client: &impl SessionClient,
) -> Result<(), PlanSessionError> {
    if !matches!(command.intent, PlanSessionIntent::Dispatch)
        || command.mode != DispatchMode::Multiplexer
    {
        return Ok(());
    }
    if !session_client.available() {
        return Err(PlanSessionError::MultiplexerNotFound);
    }
    let session_name = plan.target.session_name();
    let session_exists = session_client
        .session_exists(&session_name)
        .map_err(|message| PlanSessionError::MultiplexerSessionCheck {
            session: session_name.clone(),
            message,
        })?;
    if session_exists {
        return Ok(());
    }
    let start = SessionStart {
        session_name: &session_name,
        working_directory: &plan.launch.project_path,
    };
    let start_command_argv = session_client.preview_start(&start);
    Err(PlanSessionError::MultiplexerSessionMissing {
        session: session_name,
        start_command_argv,
    })
}

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

fn thread_title(
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

fn launch_prompt(
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

fn dispatch_target(task_id: &TaskId) -> DispatchTarget {
    DispatchTarget {
        task_id: task_id.clone(),
    }
}

#[derive(Debug, Error)]
enum ModelSelectionError {
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

fn resolve_model<E>(
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
    fn target_preserves_the_typed_task_id() {
        let task_id = "cfg9".parse::<TaskId>().unwrap();
        let target = dispatch_target(&task_id);

        assert_eq!(target.task_id, task_id);
    }
}

#[cfg(test)]
mod model_selection_tests {
    use std::{assert_matches, error::Error, fmt};

    use pwf_models::task::{EffortTier, TaskId};
    use pwf_wire::task::session::{ModelTier, ModelTierLookup};

    use super::{ModelSelectionError, parse_effort, resolve_model};
    use crate::task::session::Agent;

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
