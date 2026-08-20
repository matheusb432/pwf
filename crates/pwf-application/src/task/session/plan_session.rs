//! Plans and host-validates a task session before dispatch.

use std::{error::Error, path::PathBuf};

use askama::Template;
use pwf_models::{
    project::{HomeDirectory, ProjectId, ProjectSourceValue},
    session::{
        AgentModel, LaunchPrompt, PushedPrompt, SessionThreadTitle, SessionWorkingDirectory,
    },
    task::{EffortTier, TaskId},
};
use pwf_wire::{
    project::{ListProjects, ProjectStatusFilter},
    task::{
        BlockedByIssue, BlockedByResolution, BlockedByStatus, TaskView,
        session::{
            AgentLaunch, DispatchConfirmation, DryRunSession, ModelTierLookup, PlanSession,
            PlanSessionIntent, PlannedSession, PreparedSessionDispatch, SessionPlan,
            SessionWarning,
        },
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
        task_record::{Materialization, StoredBlockedBy, TaskRecord, TaskStore},
    },
    project::{list_projects, runtime_path},
    task::{active_task, blocked_by},
};

#[derive(Debug, Error)]
pub enum PlanSessionError {
    #[error("{0}")]
    FindTask(#[source] Box<dyn Error + Send + Sync>),
    #[error("{0}")]
    ReadTaskMarkdown(#[source] Box<dyn Error + Send + Sync>),
    #[error("Task '{id}' is not launchable: {launch}")]
    NotLaunchable {
        id: TaskId,
        launch: pwf_wire::task::TaskLaunch,
    },
    #[error("Project path for '{project_id}' does not exist: {path}")]
    ProjectPathMissing {
        project_id: ProjectId,
        path: SessionWorkingDirectory,
    },
    #[error("Session multiplexer is unavailable; cannot dispatch a pwf session.")]
    MultiplexerNotFound,
    #[error("Checking multiplexer session '{session}' failed: {source}")]
    MultiplexerSessionCheck {
        session: String,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
    #[error("Multiplexer session '{session}' does not exist")]
    MultiplexerSessionMissing {
        session: String,
        start_command_argv: Vec<String>,
    },
    #[error("{0}")]
    ModelTier(#[source] Box<dyn Error + Send + Sync>),
    #[error("Failed to render session title: {0}")]
    RenderThreadTitle(#[source] askama::Error),
    #[error("Invalid path for project '{project_id}': {source}")]
    InvalidProjectPath {
        project_id: ProjectId,
        #[source]
        source: SessionProjectPathError,
    },
    #[error("Agent command is empty.")]
    EmptyAgentCommand,
}

#[derive(Debug, Error)]
pub enum SessionProjectPathError {
    #[error(transparent)]
    Runtime(#[from] runtime_path::RuntimePathError),
    #[error("resolved path is not valid Unicode")]
    NonUnicode,
}

/// Plans one active task without mutating its note or dispatching an agent.
///
/// # Errors
///
/// Returns [`PlanSessionError`] for lookup, launch validation, or model selection failures.
#[cqrsy::command]
pub async fn execute(
    command: &PlanSession,
    store: &(impl TaskStore + ProjectNoteStore),
    pool: &sqlx::SqlitePool,
    home: &HomeDirectory,
    agent_client: &impl AgentClient,
    project_directory: &impl ProjectDirectoryClient,
    session_client: &impl SessionClient,
) -> Result<PlannedSession, PlanSessionError> {
    let probe = agent_client.probe(command.agent);
    let found = active_task::find(&command.task_id, store, pool)
        .await
        .map_err(|error| PlanSessionError::FindTask(Box::new(error)))?;
    let task = found.task;
    if !task.launch.is_ready() {
        return Err(PlanSessionError::NotLaunchable {
            id: command.task_id.clone(),
            launch: task.launch.clone(),
        });
    }
    let warnings = blocker_warnings(&found.record, store, pool).await;
    let project_id = found.project.id.clone();
    let project_path = resolve_project_path(found.project.source.value(), &project_id, home)?;
    let task_content = load_task_content(&found.record, store)?;

    let model = match command.model_override.as_deref() {
        Some(_) => command.model_override.clone(),
        None => AgentModel::from(
            resolve_model(command.agent, task.effort, |effort| {
                agent_client.model_tier(effort)
            })
            .map_err(|error| PlanSessionError::ModelTier(Box::new(error)))?,
        ),
    };
    let plan = SessionPlan {
        launch: AgentLaunch {
            agent: command.agent,
            task_id: command.task_id.clone(),
            title: SessionThreadTitle::new(
                thread_title(
                    &task,
                    &command.task_id,
                    command.directives,
                    command.agent,
                    command.effort,
                )
                .map_err(PlanSessionError::RenderThreadTitle)?,
            ),
            project_path: project_path.clone(),
            prompt: LaunchPrompt::new(launch_prompt(
                &task_content,
                &command.task_id,
                command.pushed_prompt.as_ref(),
                command.directives,
            )),
            model: model.clone(),
            effort: command.effort,
        },
        mode: command.mode,
    };
    validate_project_path(project_directory, &project_id, &plan.launch.project_path)?;
    validate_multiplexer(command, &plan, session_client)?;
    let confirmation = DispatchConfirmation {
        task_id: command.task_id.clone(),
        title: task.heading,
        created: task.created,
        mode: command.mode,
        agent: command.agent,
        directives: command.directives,
        has_pushed_prompt: command.pushed_prompt.is_some(),
        model,
        effort: command.effort,
    };

    match command.intent {
        PlanSessionIntent::Dispatch => Ok(PlannedSession::Dispatch(PreparedSessionDispatch {
            plan,
            confirmation,
            probe,
            warnings,
        })),
        PlanSessionIntent::DryRun => {
            let provider_argv = agent_client.preview(&plan.launch);
            let argv = preview_dispatch_argv(provider_argv, &plan, session_client)?;
            Ok(PlannedSession::DryRun(DryRunSession {
                plan,
                argv,
                probe,
                warnings,
            }))
        }
    }
}

async fn blocker_warnings(
    record: &TaskRecord,
    store: &impl TaskStore,
    pool: &sqlx::SqlitePool,
) -> Vec<SessionWarning> {
    let blocked_by = match &record.blocked_by {
        StoredBlockedBy::Absent => return Vec::new(),
        StoredBlockedBy::Malformed { raw, reason } => {
            return vec![SessionWarning::BlockedByMetadata(
                BlockedByIssue::Malformed {
                    path: record.locator.clone(),
                    raw: raw.clone(),
                    reason: reason.clone(),
                },
            )];
        }
        StoredBlockedBy::Valid(blocked_by) => blocked_by,
    };
    let statuses = match list_projects::execute(
        ListProjects {
            status: ProjectStatusFilter::IncludingPaused,
        },
        pool,
    )
    .await
    {
        Ok(projects) => blocked_by::statuses(blocked_by, store, None, &projects),
        Err(error) => blocked_by
            .iter()
            .map(|id| BlockedByStatus {
                id: id.clone(),
                title: None,
                resolution: BlockedByResolution::Unavailable {
                    reason: error.to_string(),
                },
            })
            .collect(),
    };
    statuses
        .into_iter()
        .filter(|status| status.resolution.is_warning())
        .map(SessionWarning::BlockedBy)
        .collect()
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
            let target = plan.target();
            let session_name = target.session_name();
            let agent_command = AgentCommand::try_new(&provider_argv)
                .map_err(|_| PlanSessionError::EmptyAgentCommand)?;
            Ok(session_client.preview_window(&SessionWindow {
                session_name: &session_name,
                working_directory: &plan.launch.project_path,
                window_name: target.task_id().as_ref(),
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
    home: &HomeDirectory,
) -> Result<SessionWorkingDirectory, PlanSessionError> {
    let resolved = runtime_path::resolve(source_value.as_ref(), home).map_err(|source| {
        PlanSessionError::InvalidProjectPath {
            project_id: project_id.clone(),
            source: source.into(),
        }
    })?;
    let path = resolved.path().to_str().map(str::to_owned).ok_or_else(|| {
        PlanSessionError::InvalidProjectPath {
            project_id: project_id.clone(),
            source: SessionProjectPathError::NonUnicode,
        }
    })?;
    Ok(SessionWorkingDirectory::new(path))
}

fn validate_project_path(
    project_directory: &impl ProjectDirectoryClient,
    project_id: &ProjectId,
    project_path: &SessionWorkingDirectory,
) -> Result<(), PlanSessionError> {
    if project_directory.is_directory(project_path) {
        return Ok(());
    }
    Err(PlanSessionError::ProjectPathMissing {
        project_id: project_id.clone(),
        path: project_path.clone(),
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
    let session_name = plan.target().session_name();
    let session_exists = session_client
        .session_exists(&session_name)
        .map_err(|source| PlanSessionError::MultiplexerSessionCheck {
            session: session_name.clone(),
            source: Box::new(source),
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
        task_title: task.heading.as_ref(),
        project: task.project.as_ref(),
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
    format!(
        "{}{}",
        project_id.as_ref().to_ascii_lowercase(),
        task_id.number()
    )
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

#[derive(Debug, Error)]
enum ModelSelectionError {
    #[error("{0}")]
    Catalog(#[source] Box<dyn Error + Send + Sync>),
    #[error("tier {tier} has no [tiers.{tier}] entry in {}", catalog.display())]
    MissingTier { tier: EffortTier, catalog: PathBuf },
    #[error("tier {tier} in {} has no claude_model set", catalog.display())]
    MissingClaudeModel { tier: EffortTier, catalog: PathBuf },
}

fn resolve_model<E>(
    agent: Agent,
    effort: Option<EffortTier>,
    model_tier: impl FnOnce(EffortTier) -> Result<ModelTierLookup, E>,
) -> Result<Option<String>, ModelSelectionError>
where
    E: Error + Send + Sync + 'static,
{
    if agent == Agent::Codex {
        return Ok(None);
    }
    let Some(tier) = effort else {
        return Ok(None);
    };
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    #[cfg(unix)]
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    use pwf_models::{
        project::{HomeDirectory, ProjectId, ProjectSourceValue},
        session::PushedPrompt,
        task::TaskId,
    };

    use super::{launch_prompt, resolve_project_path};
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

    #[cfg(unix)]
    #[test]
    fn non_unicode_runtime_project_path_is_rejected_before_session_planning() {
        let home = HomeDirectory::new(PathBuf::from(OsString::from_vec(vec![
            b'/', b'h', b'o', b'm', b'e', b'/', 0xff,
        ])));
        let source = ProjectSourceValue::try_new("~").unwrap();
        let project_id = ProjectId::try_new("PWF").unwrap();

        let error = resolve_project_path(&source, &project_id, &home).unwrap_err();

        assert!(matches!(
            error,
            super::PlanSessionError::InvalidProjectPath { .. }
        ));
        assert!(error.to_string().contains("not valid Unicode"));
    }

    #[test]
    fn invalid_runtime_project_path_retains_the_resolution_error() {
        let home = HomeDirectory::new(PathBuf::from("/home/dev"));
        let source = ProjectSourceValue::try_new("~/../pwf").unwrap();
        let project_id = ProjectId::try_new("PWF").unwrap();

        let error = resolve_project_path(&source, &project_id, &home).unwrap_err();

        assert!(matches!(
            error,
            super::PlanSessionError::InvalidProjectPath {
                source: super::SessionProjectPathError::Runtime(_),
                ..
            }
        ));
    }
}

#[cfg(test)]
mod model_selection_tests {
    use std::{assert_matches, error::Error, fmt};

    use pwf_models::task::EffortTier;
    use pwf_wire::task::session::{ModelTier, ModelTierLookup};

    use super::{ModelSelectionError, resolve_model};
    use crate::task::session::Agent;

    const CATALOG_PATH: &str = "/config/model-tiers.toml";

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
            catalog: CATALOG_PATH.into(),
            tier: Some(ModelTier {
                claude_model: claude_model.map(str::to_string),
            }),
        }
    }

    #[test]
    fn codex_ignores_effort_and_the_catalog() {
        let model = resolve_model(Agent::Codex, Some(EffortTier::Highest), |_| {
            Err(CatalogError("catalog unavailable"))
        })
        .unwrap();

        assert_eq!(model, None);
    }

    #[test]
    fn claude_without_effort_does_not_read_the_catalog() {
        let model = resolve_model(Agent::Claude, None, |_| {
            Err(CatalogError("catalog unavailable"))
        })
        .unwrap();

        assert_eq!(model, None);
    }

    #[test]
    fn configured_claude_model_is_selected() {
        let model = resolve_model(Agent::Claude, Some(EffortTier::High), |_| {
            Ok::<_, CatalogError>(catalog(Some("sonnet")))
        })
        .unwrap();

        assert_eq!(model.as_deref(), Some("sonnet"));
    }

    #[test]
    fn empty_claude_model_is_the_no_override_sentinel() {
        let model = resolve_model(Agent::Claude, Some(EffortTier::Medium), |_| {
            Ok::<_, CatalogError>(catalog(Some("")))
        })
        .unwrap();

        assert_eq!(model, None);
    }

    #[test]
    fn catalog_read_error_retains_its_source() {
        let error = resolve_model(Agent::Claude, Some(EffortTier::Low), |_| {
            Err(CatalogError("catalog unavailable"))
        })
        .unwrap_err();

        assert_eq!(error.source().unwrap().to_string(), "catalog unavailable");
    }

    #[test]
    fn missing_tier_is_an_application_error() {
        let error = resolve_model(Agent::Claude, Some(EffortTier::Highest), |_| {
            Ok::<_, CatalogError>(ModelTierLookup {
                catalog: CATALOG_PATH.into(),
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
        let error = resolve_model(Agent::Claude, Some(EffortTier::Highest), |_| {
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

#[cfg(test)]
mod blocker_warning_tests {
    use pwf_models::task::TaskStatus;
    use pwf_wire::task::{
        BlockedByIssue, BlockedByResolution, BlockedByStatus, session::SessionWarning,
    };

    use super::blocker_warnings;
    use crate::{
        ports::task_record::{StoredBlockedBy, TaskRecord},
        testing::{InMemoryStore, insert_project, stored_blocked_by, task_record},
    };

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn direct_blocker_warnings_include_unresolved_missing_and_malformed_data(
        pool: sqlx::SqlitePool,
    ) {
        insert_project(
            &pool,
            "AUX",
            "paused-project",
            "/projects/paused",
            "/tasks/paused",
            true,
        )
        .await;
        let done = TaskRecord {
            status: TaskStatus::Done,
            ..task_record("AUX-0001")
        };
        let active = task_record("AUX-0002");
        let cancelled = TaskRecord {
            status: TaskStatus::Cancelled,
            ..task_record("AUX-0003")
        };
        let store =
            InMemoryStore::default().with_project("paused-project", vec![done, active, cancelled]);
        let target = TaskRecord {
            blocked_by: stored_blocked_by(&["AUX-0001", "AUX-0002", "AUX-0003", "AUX-9999"]),
            ..task_record("PWF-0001")
        };

        let warnings = blocker_warnings(&target, &store, &pool).await;

        assert_eq!(
            warnings,
            [
                SessionWarning::BlockedBy(BlockedByStatus {
                    id: "AUX-0002".parse().unwrap(),
                    title: Some("tray gui".to_string()),
                    resolution: BlockedByResolution::Found(TaskStatus::Active),
                }),
                SessionWarning::BlockedBy(BlockedByStatus {
                    id: "AUX-0003".parse().unwrap(),
                    title: Some("tray gui".to_string()),
                    resolution: BlockedByResolution::Found(TaskStatus::Cancelled),
                }),
                SessionWarning::BlockedBy(BlockedByStatus {
                    id: "AUX-9999".parse().unwrap(),
                    title: None,
                    resolution: BlockedByResolution::Missing,
                }),
            ]
        );

        let malformed = TaskRecord {
            blocked_by: StoredBlockedBy::Malformed {
                raw: "\"[[AUX-0001]]\"".to_string(),
                reason: "expected a sequence".to_string(),
            },
            ..task_record("PWF-0001")
        };
        assert_eq!(
            blocker_warnings(&malformed, &store, &pool).await,
            [SessionWarning::BlockedByMetadata(
                BlockedByIssue::Malformed {
                    path: malformed.locator,
                    raw: "\"[[AUX-0001]]\"".to_string(),
                    reason: "expected a sequence".to_string(),
                }
            )]
        );
    }
}
