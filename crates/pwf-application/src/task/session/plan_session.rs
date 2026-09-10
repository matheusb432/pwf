//! Plans and host-validates a task session before dispatch.

use askama::Template;
use pwf_models::{
    project::{HomeDirectory, Project, ProjectId, ProjectSourceValue},
    session::{LaunchPrompt, PushedPrompt, SessionThreadTitle, SessionWorkingDirectory},
    task::TaskId,
};
use pwf_wire::task::{
    BlockedByIssue, BlockedByResolution, BlockedByStatus, StoredBlockedBy, TaskHeading, TaskRecord,
    session::{
        AgentLaunch, DispatchConfirmation, DryRunSession, PlanSession, PlanSessionIntent,
        PlannedSession, PreparedSessionDispatch, PreparedTaskRevision, SessionPlan, SessionWarning,
    },
};
use thiserror::Error;

use super::{Agent, SessionEffort};
use crate::{
    ports::{agent::AgentClient, project_directory::ProjectDirectoryClient, task_vault::TaskVault},
    project::{get_projects, runtime_path},
    task::{active_task, blocked_by},
};

#[derive(Debug, Error)]
pub enum PlanSessionError {
    #[error(transparent)]
    FindTask(anyhow::Error),
    #[error("Task '{id}' is not launchable: {launch}")]
    NotLaunchable {
        id: TaskId,
        launch: pwf_wire::task::TaskLaunch,
    },
    #[error(
        "project '{project_id}' has no source path; set one with `pwf project edit {project_id} --source <path>` before starting a session"
    )]
    ProjectSourceMissing { project_id: ProjectId },
    #[error("Project path for '{project_id}' does not exist: {path}")]
    ProjectPathMissing {
        project_id: ProjectId,
        path: SessionWorkingDirectory,
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

pub struct SessionPlanningClients<A, P> {
    pub(crate) agent: A,
    pub(crate) project_directory: P,
}

impl<A, P> SessionPlanningClients<A, P> {
    #[must_use]
    pub const fn new(agent: A, project_directory: P) -> Self {
        Self {
            agent,
            project_directory,
        }
    }
}

struct PlannedTask {
    heading: TaskHeading,
    created: Option<pwf_models::AppDate>,
    content: String,
    revision: PreparedTaskRevision,
}

/// Plans active tasks without mutating their notes or dispatching an agent.
#[cqrsy::command]
pub async fn execute(
    command: &PlanSession,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
    home: &HomeDirectory,
    clients: &SessionPlanningClients<impl AgentClient, impl ProjectDirectoryClient>,
) -> Result<PlannedSession, PlanSessionError> {
    let (project, first_task, mut warnings) =
        plan_task(command.task_ids.first(), store, pool).await?;
    let first_title = first_task.heading.clone();
    let first_created = first_task.created;
    let singleton_thread_title = if command.task_ids.is_singleton() {
        Some(
            thread_title(
                &first_task.heading,
                &project.title,
                command.task_ids.first(),
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
    let source = project
        .source
        .as_ref()
        .ok_or_else(|| PlanSessionError::ProjectSourceMissing {
            project_id: project_id.clone(),
        })?;
    let project_path = resolve_project_path(source.value(), &project_id, home)?;
    let probe = clients.agent.probe(command.agent);
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
                command.pushed_prompt.as_ref(),
            )),
            model: model.clone(),
            effort: command.effort,
        },
    };
    validate_project_path(
        &clients.project_directory,
        &project_id,
        &plan.launch.project_path,
    )?;
    let confirmation = DispatchConfirmation {
        task_ids: command.task_ids.clone(),
        title: first_title,
        created: first_created,
        agent: command.agent,
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
            let argv = preview_dispatch_argv(provider_argv)?;
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
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
) -> Result<(Project, PlannedTask, Vec<SessionWarning>), PlanSessionError> {
    let found = active_task::find(task_id, store, pool)
        .await
        .map_err(|error| PlanSessionError::FindTask(anyhow::Error::new(error)))?;
    let launch = crate::task::task_projection::derive_flags(&pwf_models::task::TaskPrompt::new(
        found.record.body.trim(),
    ));
    if !launch.is_ready() {
        return Err(PlanSessionError::NotLaunchable {
            id: task_id.clone(),
            launch,
        });
    }
    let warnings = blocker_warnings(&found.record, store, pool).await;
    let content = found.record.source.clone();
    let revision = PreparedTaskRevision {
        task_id: found.record.id.clone(),
        revision: found.record.revision.clone(),
    };
    Ok((
        found.project,
        PlannedTask {
            heading: found.heading,
            created: found
                .record
                .created_at
                .map(pwf_models::task::TaskTimestamp::date),
            content,
            revision,
        },
        warnings,
    ))
}

async fn blocker_warnings(
    record: &TaskRecord,
    store: &impl TaskVault,
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
    let project_ids = blocked_by
        .iter()
        .map(|id| id.project_id().clone())
        .collect();
    let statuses = match get_projects::execute(&project_ids, pool).await {
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

fn preview_dispatch_argv(provider_argv: Vec<String>) -> Result<Vec<String>, PlanSessionError> {
    if provider_argv.is_empty() {
        return Err(PlanSessionError::EmptyAgentCommand);
    }
    Ok(provider_argv)
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
    if project_directory.is_directory(std::path::Path::new(project_path.as_ref())) {
        return Ok(());
    }
    Err(PlanSessionError::ProjectPathMissing {
        project_id: project_id.clone(),
        path: project_path.clone(),
    })
}

const SESSION_CONTEXT_OPEN: &str = "<pwf_session_context>\n";
const SESSION_CONTEXT_CLOSE: &str = "\n</pwf_session_context>";
const PWF_TASK_OPEN: &str = "<pwf_task>\n";
const PWF_TASK_CLOSE: &str = "</pwf_task>";

fn session_context_rendered_len(pushed_prompt: Option<&PushedPrompt>) -> usize {
    let Some(pushed_prompt) = pushed_prompt else {
        return 0;
    };

    SESSION_CONTEXT_OPEN.len() + SESSION_CONTEXT_CLOSE.len() + pushed_prompt.as_ref().len()
}

fn write_session_context(output: &mut String, pushed_prompt: Option<&PushedPrompt>) {
    let Some(pushed_prompt) = pushed_prompt else {
        return;
    };

    output.push_str(SESSION_CONTEXT_OPEN);
    output.push_str(pushed_prompt.as_ref());
    output.push_str(SESSION_CONTEXT_CLOSE);
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
    agent: &'static str,
}

fn thread_title(
    heading: &TaskHeading,
    project: &pwf_models::project::ProjectName,
    task_id: &TaskId,
    agent: Agent,
    effort: SessionEffort,
) -> Result<String, askama::Error> {
    ThreadTitleTemplate {
        task_id,
        task_id_brief: task_id_brief(task_id),
        task_title: heading.as_ref(),
        project: project.as_ref(),
        effort,
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

fn launch_prompt(task_contents: &[&str], pushed_prompt: Option<&PushedPrompt>) -> String {
    let session_context_len = session_context_rendered_len(pushed_prompt);
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
    write_session_context(&mut prompt, pushed_prompt);
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
        session::PushedPrompt,
    };

    use super::{launch_prompt, resolve_project_path};

    #[test]
    fn launch_prompt_wraps_the_task_without_rendering_an_empty_session_context() {
        assert_eq!(
            launch_prompt(&["task content"], None),
            "<pwf_task>\ntask content\n</pwf_task>"
        );
    }

    #[test]
    fn launch_prompt_orders_all_session_context_content_before_the_task() {
        let pushed_prompt = PushedPrompt::try_new("extra context").unwrap();

        assert_eq!(
            launch_prompt(&["task content"], Some(&pushed_prompt)),
            concat!(
                "<pwf_session_context>\n",
                "extra context\n",
                "</pwf_session_context>\n\n",
                "<pwf_task>\n",
                "task content\n",
                "</pwf_task>",
            )
        );
    }

    #[test]
    fn task_wrapper_preserves_an_existing_trailing_newline_without_adding_a_blank_line() {
        assert_eq!(
            launch_prompt(&["task content\n"], None),
            "<pwf_task>\ntask content\n</pwf_task>"
        );
    }

    #[test]
    fn launch_prompt_wraps_multiple_tasks_in_supplied_order_after_one_context() {
        let pushed_prompt = PushedPrompt::try_new("shared context").unwrap();

        assert_eq!(
            launch_prompt(&["task twenty-three", "task fifteen"], Some(&pushed_prompt),),
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
