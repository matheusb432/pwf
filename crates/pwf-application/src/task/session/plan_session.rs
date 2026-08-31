//! Plans and host-validates a task session before dispatch.

use askama::Template;
use pwf_models::{
    project::{HomeDirectory, Project, ProjectId, ProjectSourceValue},
    session::{
        LaunchPrompt, PushedPrompt, SessionTaskIds, SessionThreadTitle, SessionWorkingDirectory,
    },
    task::TaskId,
};
use pwf_wire::{
    project::ProjectStatusFilter,
    task::{
        BlockedByIssue, BlockedByResolution, BlockedByStatus, TaskView,
        session::{
            AgentLaunch, DispatchConfirmation, DryRunSession, PlanSession, PlanSessionIntent,
            PlannedSession, PreparedSessionDispatch, PreparedTaskRevision, SessionPlan,
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
    #[error(transparent)]
    FindTask(anyhow::Error),
    #[error(transparent)]
    ReadTaskMarkdown(anyhow::Error),
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
        source: anyhow::Error,
    },
    #[error("Multiplexer session '{session}' does not exist")]
    MultiplexerSessionMissing {
        session: String,
        start_command_argv: Vec<String>,
    },
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

pub struct SessionPlanningClients<A, P, S> {
    pub(crate) agent: A,
    pub(crate) project_directory: P,
    pub(crate) session: S,
}

impl<A, P, S> SessionPlanningClients<A, P, S> {
    #[must_use]
    pub const fn new(agent: A, project_directory: P, session: S) -> Self {
        Self {
            agent,
            project_directory,
            session,
        }
    }
}

struct PlannedTask {
    view: TaskView,
    content: String,
    revision: PreparedTaskRevision,
}

/// Plans active tasks without mutating their notes or dispatching an agent.
#[cqrsy::command]
pub async fn execute(
    command: &PlanSession,
    store: &(impl TaskStore + ProjectNoteStore),
    pool: &sqlx::SqlitePool,
    home: &HomeDirectory,
    clients: &SessionPlanningClients<
        impl AgentClient,
        impl ProjectDirectoryClient,
        impl SessionClient,
    >,
) -> Result<PlannedSession, PlanSessionError> {
    let probe = clients.agent.probe(command.agent);
    let (project, first_task, mut warnings) =
        plan_task(command.task_ids.first(), store, pool).await?;
    let first_title = first_task.view.heading.clone();
    let first_created = first_task.view.created;
    let singleton_thread_title = if command.task_ids.is_singleton() {
        Some(
            thread_title(
                &first_task.view,
                command.task_ids.first(),
                command.directives,
                command.agent,
                command.effort,
            )
            .map_err(PlanSessionError::RenderThreadTitle)?,
        )
    } else {
        None
    };
    let mut tasks = vec![first_task];
    for task_id in command.task_ids.iter().skip(1) {
        let (resolved_project, task, task_warnings) = plan_task(task_id, store, pool).await?;
        debug_assert_eq!(resolved_project.id, project.id);
        tasks.push(task);
        warnings.extend(task_warnings);
    }

    let project_id = project.id.clone();
    let project_path = resolve_project_path(project.source.value(), &project_id, home)?;
    let task_contents = tasks
        .iter()
        .map(|task| task.content.as_str())
        .collect::<Vec<_>>();

    let model = command.model_override.clone();
    let plan = SessionPlan {
        launch: AgentLaunch {
            agent: command.agent,
            task_ids: command.task_ids.clone(),
            title: SessionThreadTitle::new(
                singleton_thread_title.unwrap_or_else(|| command.task_ids.identity()),
            ),
            project_path: project_path.clone(),
            prompt: LaunchPrompt::new(launch_prompt(
                &task_contents,
                &command.task_ids,
                command.pushed_prompt.as_ref(),
                command.directives,
            )),
            model: model.clone(),
            effort: command.effort,
        },
        mode: command.mode,
    };
    validate_project_path(
        &clients.project_directory,
        &project_id,
        &plan.launch.project_path,
    )?;
    validate_multiplexer(command, &plan, &clients.session)?;
    let confirmation = DispatchConfirmation {
        task_ids: command.task_ids.clone(),
        title: first_title,
        created: first_created,
        mode: command.mode,
        agent: command.agent,
        directives: command.directives,
        has_pushed_prompt: command.pushed_prompt.is_some(),
        model,
        effort: command.effort,
    };

    match command.intent {
        PlanSessionIntent::Dispatch => Ok(PlannedSession::Dispatch(PreparedSessionDispatch {
            project: Box::new(project),
            plan,
            confirmation,
            probe,
            warnings,
            task_revisions: tasks.iter().map(|task| task.revision.clone()).collect(),
        })),
        PlanSessionIntent::DryRun => {
            let provider_argv = clients.agent.preview(&plan.launch);
            let argv = preview_dispatch_argv(provider_argv, &plan, &clients.session)?;
            Ok(PlannedSession::DryRun(DryRunSession {
                plan,
                argv,
                probe,
                warnings,
            }))
        }
    }
}

async fn plan_task(
    task_id: &TaskId,
    store: &(impl TaskStore + ProjectNoteStore),
    pool: &sqlx::SqlitePool,
) -> Result<(Project, PlannedTask, Vec<SessionWarning>), PlanSessionError> {
    let found = active_task::find(task_id, store, pool)
        .await
        .map_err(|error| PlanSessionError::FindTask(anyhow::Error::new(error)))?;
    if !found.task.launch.is_ready() {
        return Err(PlanSessionError::NotLaunchable {
            id: task_id.clone(),
            launch: found.task.launch,
        });
    }
    let warnings = blocker_warnings(&found.record, store, pool).await;
    let content = load_task_content(&found.record, store)?;
    let revision = PreparedTaskRevision {
        task_id: found.record.id.clone(),
        revision: found.record.revision.clone(),
    };
    Ok((
        found.project,
        PlannedTask {
            view: found.task,
            content,
            revision,
        },
        warnings,
    ))
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
    let statuses = match list_projects::execute(ProjectStatusFilter::IncludingPaused, pool).await {
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
            let session_name = target.multiplexer_session_name();
            let window_name = target.window_name();
            let agent_command = AgentCommand::try_new(&provider_argv)
                .map_err(|_| PlanSessionError::EmptyAgentCommand)?;
            Ok(session_client.preview_window(&SessionWindow {
                session_name: &session_name,
                working_directory: &plan.launch.project_path,
                window_name: &window_name,
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
            .map_err(|error| PlanSessionError::ReadTaskMarkdown(anyhow::Error::new(error)));
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
    let session_name = plan.target().multiplexer_session_name();
    let session_exists = session_client
        .session_exists(&session_name)
        .map_err(|source| PlanSessionError::MultiplexerSessionCheck {
            session: session_name.clone(),
            source: anyhow::Error::new(source),
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
const COMPOUND_WORKTREE_DIRECTIVE_SUFFIX: &str = "` (the worktree name is this session's sorted task identity), and do all of these tasks' work inside that worktree.";

fn worktree_directive_suffix(task_ids: &SessionTaskIds) -> &'static str {
    if task_ids.is_singleton() {
        WORKTREE_DIRECTIVE_SUFFIX
    } else {
        COMPOUND_WORKTREE_DIRECTIVE_SUFFIX
    }
}

fn session_context_rendered_len(
    task_ids: &SessionTaskIds,
    pushed_prompt: Option<&PushedPrompt>,
    directives: LaunchDirectives,
) -> usize {
    let content_count = usize::from(pushed_prompt.is_some())
        + usize::from(directives.autonomous)
        + usize::from(directives.worktree);
    if content_count == 0 {
        return 0;
    }

    let worktree_name = task_ids.identity();
    let worktree_suffix = worktree_directive_suffix(task_ids);
    SESSION_CONTEXT_OPEN.len()
        + SESSION_CONTEXT_CLOSE.len()
        + pushed_prompt.map_or(0, |prompt| prompt.as_ref().len())
        + usize::from(directives.autonomous) * AUTONOMY_DIRECTIVE.len()
        + usize::from(directives.worktree)
            * (WORKTREE_DIRECTIVE_PREFIX.len() + worktree_name.len() + worktree_suffix.len())
        + (content_count - 1) * 2
}

fn write_session_context(
    output: &mut String,
    task_ids: &SessionTaskIds,
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
        output.push_str(&task_ids.identity());
        output.push_str(worktree_directive_suffix(task_ids));
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
    task_contents: &[&str],
    task_ids: &SessionTaskIds,
    pushed_prompt: Option<&PushedPrompt>,
    directives: LaunchDirectives,
) -> String {
    let session_context_len = session_context_rendered_len(task_ids, pushed_prompt, directives);
    let separator_len = usize::from(session_context_len > 0) * 2;
    let tasks_len = task_contents
        .iter()
        .map(|task_content| {
            PWF_TASK_OPEN.len()
                + task_content.len()
                + usize::from(!task_content.ends_with('\n'))
                + PWF_TASK_CLOSE.len()
        })
        .sum::<usize>()
        + task_contents.len().saturating_sub(1) * 2;
    let prompt_len = session_context_len + separator_len + tasks_len;
    let mut prompt = String::with_capacity(prompt_len);
    write_session_context(&mut prompt, task_ids, pushed_prompt, directives);
    if session_context_len > 0 {
        prompt.push_str("\n\n");
    }
    for (index, task_content) in task_contents.iter().enumerate() {
        if index > 0 {
            prompt.push_str("\n\n");
        }
        prompt.push_str(PWF_TASK_OPEN);
        prompt.push_str(task_content);
        if !task_content.ends_with('\n') {
            prompt.push('\n');
        }
        prompt.push_str(PWF_TASK_CLOSE);
    }
    debug_assert_eq!(prompt.len(), prompt_len);
    prompt
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    #[cfg(unix)]
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    use pwf_models::{
        project::{HomeDirectory, ProjectId, ProjectSourceValue},
        session::{PushedPrompt, SessionTaskIds},
        task::TaskId,
    };

    use super::{launch_prompt, resolve_project_path};
    use crate::task::session::LaunchDirectives;

    #[test]
    fn launch_prompt_wraps_the_task_without_rendering_an_empty_session_context() {
        let task_id = TaskId::try_new("FOO-0001").unwrap();

        assert_eq!(
            launch_prompt(
                &["task content"],
                &SessionTaskIds::try_new([task_id]).unwrap(),
                None,
                LaunchDirectives::default(),
            ),
            "<pwf_task>\ntask content\n</pwf_task>"
        );
    }

    #[test]
    fn launch_prompt_orders_all_session_context_content_before_the_task() {
        let task_id = TaskId::try_new("FOO-0001").unwrap();
        let pushed_prompt = PushedPrompt::try_new("extra context").unwrap();

        assert_eq!(
            launch_prompt(
                &["task content"],
                &SessionTaskIds::try_new([task_id]).unwrap(),
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
                "Workspace: before doing anything else, use a git-worktrees skill to create a git worktree here named `FOO-0001` (the worktree name is this task's id), and do all of this task's work inside that worktree.\n",
                "</pwf_session_context>\n\n",
                "<pwf_task>\n",
                "task content\n",
                "</pwf_task>",
            )
        );
    }

    #[test]
    fn task_wrapper_preserves_an_existing_trailing_newline_without_adding_a_blank_line() {
        let task_id = TaskId::try_new("FOO-0001").unwrap();

        assert_eq!(
            launch_prompt(
                &["task content\n"],
                &SessionTaskIds::try_new([task_id]).unwrap(),
                None,
                LaunchDirectives::default()
            ),
            "<pwf_task>\ntask content\n</pwf_task>"
        );
    }

    #[test]
    fn launch_prompt_wraps_multiple_tasks_in_supplied_order_after_one_context() {
        let task_ids =
            SessionTaskIds::try_new(["foo23".parse().unwrap(), "foo15".parse().unwrap()]).unwrap();
        let pushed_prompt = PushedPrompt::try_new("shared context").unwrap();

        assert_eq!(
            launch_prompt(
                &["task twenty-three", "task fifteen"],
                &task_ids,
                Some(&pushed_prompt),
                LaunchDirectives::default(),
            ),
            concat!(
                "<pwf_session_context>\n",
                "shared context\n",
                "</pwf_session_context>\n\n",
                "<pwf_task>\n",
                "task twenty-three\n",
                "</pwf_task>\n\n",
                "<pwf_task>\n",
                "task fifteen\n",
                "</pwf_task>",
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_runtime_project_path_is_rejected_before_session_planning() {
        let home = HomeDirectory::new(PathBuf::from(OsString::from_vec(vec![
            b'/', b'h', b'o', b'm', b'e', b'/', 0xff,
        ])));
        let source = ProjectSourceValue::try_new("~").unwrap();
        let project_id = ProjectId::try_new("FOO").unwrap();

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
        let source = ProjectSourceValue::try_new("~/../foo").unwrap();
        let project_id = ProjectId::try_new("FOO").unwrap();

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
mod planned_model_tests {
    use std::convert::Infallible;

    use pwf_models::{
        project::HomeDirectory,
        session::{
            Agent, AgentModel, DispatchMode, LaunchDirectives, SessionEffort, SessionTaskIds,
            SessionWorkingDirectory,
        },
        task::EffortTier,
    };
    use pwf_wire::task::session::{
        AgentAvailability, AgentLaunch, AgentProbe, PlanSession, PlanSessionIntent, PlannedSession,
    };

    use super::SessionPlanningClients;
    use crate::{
        ports::{
            agent::{AgentClient, PreparedAgentLaunch},
            project_directory::ProjectDirectoryClient,
            session::{SessionClient, SessionStart, SessionWindow},
            task_record::TaskRecord,
        },
        task::session::plan_session,
        testing::{InMemoryStore, insert_project, task_record},
    };

    #[derive(Clone, Copy)]
    struct AgentStub;

    impl AgentClient for AgentStub {
        type PreparationError = Infallible;

        fn probe(&self, agent: Agent) -> AgentProbe {
            AgentProbe {
                agent,
                availability: AgentAvailability::Available,
            }
        }

        fn preview(&self, _: &AgentLaunch) -> Vec<String> {
            vec!["claude".to_string()]
        }

        fn prepare(&self, _: &AgentLaunch) -> Result<PreparedAgentLaunch, Self::PreparationError> {
            Ok(PreparedAgentLaunch::Process {
                arguments: vec!["claude".to_string()],
            })
        }
    }

    #[derive(Clone, Copy)]
    struct ExistingProjectDirectory;

    impl ProjectDirectoryClient for ExistingProjectDirectory {
        fn is_directory(&self, _: &SessionWorkingDirectory) -> bool {
            true
        }
    }

    #[derive(Clone, Copy)]
    struct UnusedSessionClient;

    impl SessionClient for UnusedSessionClient {
        type Error = Infallible;

        fn available(&self) -> bool {
            false
        }

        fn session_exists(&self, _: &str) -> Result<bool, Self::Error> {
            Ok(false)
        }

        fn preview_start(&self, _: &SessionStart<'_>) -> Vec<String> {
            Vec::new()
        }

        fn preview_window(&self, _: &SessionWindow<'_>) -> Vec<String> {
            Vec::new()
        }

        fn open_window(&self, _: &SessionWindow<'_>) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    fn command(model_override: Option<&str>) -> PlanSession {
        PlanSession {
            task_ids: SessionTaskIds::try_new(["FOO-0001".parse().unwrap()]).unwrap(),
            intent: PlanSessionIntent::DryRun,
            pushed_prompt: None,
            mode: DispatchMode::Inline,
            directives: LaunchDirectives::default(),
            agent: Agent::Claude,
            model_override: AgentModel::from(model_override.map(str::to_string)),
            effort: SessionEffort::Max,
        }
    }

    async fn planned_model(
        command: &PlanSession,
        store: &InMemoryStore,
        pool: &sqlx::SqlitePool,
        clients: &SessionPlanningClients<AgentStub, ExistingProjectDirectory, UnusedSessionClient>,
    ) -> anyhow::Result<(AgentModel, SessionEffort)> {
        let planned = plan_session::execute(
            command,
            store,
            pool,
            &HomeDirectory::new("/home/dev".into()),
            clients,
        )
        .await?;
        let dry_run = match planned {
            PlannedSession::DryRun(dry_run) => dry_run,
            PlannedSession::Dispatch(_) => {
                return Err(anyhow::anyhow!("test command must produce a dry-run plan"));
            }
        };
        Ok((dry_run.plan.launch.model, dry_run.plan.launch.effort))
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn planning_uses_only_the_explicit_model_override(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
        let record = TaskRecord {
            effort: Some(EffortTier::Highest.to_string()),
            ..task_record("FOO-0001")
        };
        let store = InMemoryStore::default().with_project("foo", vec![record]);
        let clients =
            SessionPlanningClients::new(AgentStub, ExistingProjectDirectory, UnusedSessionClient);

        let (default_model, effort) = planned_model(&command(None), &store, &pool, &clients)
            .await
            .unwrap();
        let (explicit_model, _) =
            planned_model(&command(Some("manual-model")), &store, &pool, &clients)
                .await
                .unwrap();

        assert_eq!(default_model.as_deref(), None);
        assert_eq!(explicit_model.as_deref(), Some("manual-model"));
        assert_eq!(effort, SessionEffort::Max);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn planning_uses_one_compound_identity_and_keeps_task_prompt_order(
        pool: sqlx::SqlitePool,
    ) -> anyhow::Result<()> {
        insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
        let first = TaskRecord {
            title: "first task".to_string(),
            source: "first task content".to_string(),
            ..task_record("FOO-0001")
        };
        let second = TaskRecord {
            title: "second task".to_string(),
            source: "second task content".to_string(),
            ..task_record("FOO-0002")
        };
        let store = InMemoryStore::default().with_project("foo", vec![first, second]);
        let clients =
            SessionPlanningClients::new(AgentStub, ExistingProjectDirectory, UnusedSessionClient);
        let mut command = command(None);
        command.task_ids =
            SessionTaskIds::try_new(["FOO-0002".parse().unwrap(), "FOO-0001".parse().unwrap()])
                .unwrap();

        let planned = plan_session::execute(
            &command,
            &store,
            &pool,
            &HomeDirectory::new("/home/dev".into()),
            &clients,
        )
        .await
        .unwrap();
        let PlannedSession::DryRun(dry_run) = planned else {
            anyhow::bail!("test command must produce a dry-run plan");
        };
        let launch = dry_run.plan.launch;
        let prompt = launch.prompt.as_ref();

        assert_eq!(launch.title.as_ref(), "foo1,foo2");
        assert_eq!(
            launch
                .task_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["FOO-0002", "FOO-0001"]
        );
        assert!(
            prompt.find("second task content").unwrap()
                < prompt.find("first task content").unwrap()
        );
        assert_eq!(prompt.matches("<pwf_task>").count(), 2);
        Ok(())
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
            ..task_record("FOO-0001")
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
            ..task_record("FOO-0001")
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
