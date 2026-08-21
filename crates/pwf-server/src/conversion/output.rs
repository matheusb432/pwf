use pwf_application::contract::{confirmation, note, project, task};
use pwf_models::{
    project::Project,
    session::{Agent, DispatchMode, SessionEffort},
    task::{EffortTier, TaskStatus},
};
use pwf_wire::v1;

pub(crate) fn project(value: &Project) -> v1::Project {
    v1::Project {
        id: value.id.to_string(),
        title: value.title.to_string(),
        source_kind: value.source.kind().to_string(),
        source_value: value.source.value().to_string(),
        tasks_kind: value.tasks.kind().to_string(),
        tasks_path: value.tasks.path().to_string(),
        created_at: value.created_at.to_string(),
        is_paused: value.is_paused,
    }
}

pub(crate) fn project_state_change(value: &project::ProjectStateChange) -> v1::ProjectStateChange {
    v1::ProjectStateChange {
        project: Some(project(&value.project)),
        changed: value.changed,
    }
}

pub(crate) fn added_note(value: &note::AddedNote) -> v1::AddedNote {
    v1::AddedNote {
        id: value.id.to_string(),
        title: value.title.to_string(),
    }
}

pub(crate) fn listed_notes(value: note::ListedNotes) -> v1::ListedNotes {
    v1::ListedNotes {
        project: value.project.to_string(),
        notes: value
            .notes
            .into_iter()
            .map(|note| v1::ListedNote {
                id: note.id.to_string(),
                title: note.title.to_string(),
            })
            .collect(),
        hidden: value.hidden as u64,
    }
}

pub(crate) fn removed_note(value: &note::RemovedNote) -> v1::RemovedNote {
    v1::RemovedNote {
        id: value.id.to_string(),
    }
}

pub(crate) fn updated_note(value: &note::UpdatedNote) -> v1::UpdatedNote {
    v1::UpdatedNote {
        id: value.id.to_string(),
        title: value.title.to_string(),
    }
}

pub(crate) fn added_task(value: task::AddedTask) -> v1::AddedTask {
    v1::AddedTask {
        id: value.id.to_string(),
        project: value.project.to_string(),
        title: value.title.to_string(),
        note_path: value.note_path.to_string(),
        created_section: value.created_section.map(|section| section.to_string()),
    }
}

pub(crate) fn add_task_failure_details(
    value: &task::AddTaskDiagnostics,
) -> v1::AddTaskFailureDetails {
    v1::AddTaskFailureDetails {
        project: value.project.to_string(),
        created_section: value.created_section.as_ref().map(ToString::to_string),
    }
}

pub(crate) fn edited_task(value: &task::EditedTask) -> v1::EditedTask {
    v1::EditedTask {
        id: value.id.to_string(),
        project: value.project.to_string(),
        title: value.title.to_string(),
    }
}

pub(crate) fn closed_task(value: task::ClosedTask) -> v1::ClosedTask {
    let action = match value.action {
        task::ClosedTaskAction::Done => v1::ClosedTaskAction::Done,
        task::ClosedTaskAction::Cancelled => v1::ClosedTaskAction::Cancelled,
    };
    v1::ClosedTask {
        id: value.id.to_string(),
        project: value.project.to_string(),
        title: value.title.to_string(),
        action: action as i32,
        evicted_ids: strings(value.evicted_ids),
        futuro_renamed_project: value
            .futuro_renamed_project
            .map(|project| project.to_string()),
        review_task: value.review_task.map(added_task),
    }
}

pub(crate) fn reopened_task(value: task::ReopenedTask) -> v1::ReopenedTask {
    match value {
        task::ReopenedTask::Reopened { id, project } => v1::ReopenedTask {
            outcome: v1::ReopenedTaskOutcome::Reopened as i32,
            id: id.to_string(),
            project: Some(project.to_string()),
        },
        task::ReopenedTask::AlreadyActive { id, project } => v1::ReopenedTask {
            outcome: v1::ReopenedTaskOutcome::AlreadyActive as i32,
            id: id.to_string(),
            project: Some(project.to_string()),
        },
        task::ReopenedTask::Aborted { id } => v1::ReopenedTask {
            outcome: v1::ReopenedTaskOutcome::Aborted as i32,
            id: id.to_string(),
            project: None,
        },
    }
}

pub(crate) fn removed_task_outcome(value: task::RemovedTaskOutcome) -> v1::RemovedTaskOutcome {
    match value {
        task::RemovedTaskOutcome::Removed(value) => v1::RemovedTaskOutcome {
            outcome: v1::RemovedTaskOutcomeKind::Removed as i32,
            task_id: value.id.to_string(),
            removed: Some(v1::RemovedTask {
                id: value.id.to_string(),
                project: value.project.to_string(),
                title: value.title.to_string(),
                deleted_path: value.deleted_path.to_string(),
                unlinked: value.unlinked.map(|path| path.to_string()),
            }),
        },
        task::RemovedTaskOutcome::Aborted { task_id } => v1::RemovedTaskOutcome {
            outcome: v1::RemovedTaskOutcomeKind::Aborted as i32,
            task_id: task_id.to_string(),
            removed: None,
        },
    }
}

pub(crate) fn task_read(value: task::TaskRead) -> v1::TaskRead {
    let value = match value {
        task::TaskRead::Markdown(value) => v1::task_read::Value::Markdown(value),
        task::TaskRead::Path(value) => v1::task_read::Value::Path(value.to_string()),
        task::TaskRead::Data(value) => v1::task_read::Value::Data(Box::new(task_data(*value))),
    };
    v1::TaskRead { value: Some(value) }
}

fn task_data(value: task::TaskData) -> v1::TaskData {
    v1::TaskData {
        id: value.id.to_string(),
        project: value.project.to_string(),
        title: value.title.to_string(),
        status: task_status(value.status) as i32,
        created: value.created.map(|date| date.to_string()),
        completed: value.completed.map(|date| date.to_string()),
        commits: value.commits.map(|commits| commits.to_string()),
        tags: value
            .tags
            .map(|tags| tags.iter().map(ToString::to_string).collect())
            .unwrap_or_default(),
        effort: value.effort.map(|effort| effort_tier(effort) as i32),
        blocked_by: value
            .blocked_by
            .map(|blocked_by| blocked_by.iter().map(ToString::to_string).collect())
            .unwrap_or_default(),
        section: value.section.map(|section| section.to_string()),
        prompt: value.prompt.to_string(),
    }
}

pub(crate) fn listed_tasks(value: task::ListedTasks) -> v1::ListedTasks {
    let status_filter = match value.status_filter {
        task::StatusFilter::Exact(TaskStatus::Active) => v1::TaskStatusFilter::Active,
        task::StatusFilter::Exact(TaskStatus::Done) => v1::TaskStatusFilter::Done,
        task::StatusFilter::Exact(TaskStatus::Cancelled) => v1::TaskStatusFilter::Cancelled,
        task::StatusFilter::All => v1::TaskStatusFilter::All,
    };
    let layout = match value.layout {
        task::ListLayout::Flat => v1::ListLayout::Flat,
        task::ListLayout::BySection => v1::ListLayout::BySection,
    };
    let detail = match value.detail {
        task::ListDetail::Summary => v1::ListDetail::Summary,
        task::ListDetail::Detailed => v1::ListDetail::Detailed,
    };
    v1::ListedTasks {
        tasks: value.tasks.into_iter().map(task_view).collect(),
        hidden: value.hidden as u64,
        project: value.project.map(|project| project.to_string()),
        project_task_path: value.project_task_path.map(|path| path.to_string()),
        status_filter: status_filter as i32,
        layout: layout as i32,
        detail: detail as i32,
    }
}

fn task_view(value: task::TaskView) -> v1::TaskView {
    v1::TaskView {
        id: value.id.to_string(),
        project: value.project.to_string(),
        status: task_status(value.status) as i32,
        heading: value.heading.to_string(),
        prompt: value.prompt.to_string(),
        project_path: value.project_path.to_string(),
        location: Some(v1::TaskLocation {
            index_path: value.location.index_path().to_string(),
            line: value.location.line().get() as u64,
        }),
        launch_issues: value.launch.issues().iter().map(task_issue).collect(),
        section: value.section.map(|section| section.to_string()),
        blocked_by: value
            .blocked_by
            .map(|blocked_by| blocked_by.iter().map(ToString::to_string).collect())
            .unwrap_or_default(),
        blocked_by_statuses: value
            .blocked_by_statuses
            .into_iter()
            .map(blocked_by_status)
            .collect(),
        blocked_by_issues: value
            .blocked_by_issues
            .into_iter()
            .map(blocked_by_issue)
            .collect(),
        effort: value.effort.map(|effort| effort_tier(effort) as i32),
        raw_tags: value.tags.map(|tags| tags.to_string()),
        created: value.created.map(|date| date.to_string()),
    }
}

fn task_issue(value: &task::TaskIssue) -> v1::TaskIssue {
    match value {
        task::TaskIssue::MissingNote { path } => v1::TaskIssue {
            kind: v1::TaskIssueKind::MissingNote as i32,
            path: Some(path.to_string()),
        },
        task::TaskIssue::PlaceholderPrompt => v1::TaskIssue {
            kind: v1::TaskIssueKind::PlaceholderPrompt as i32,
            path: None,
        },
    }
}

fn blocked_by_status(value: task::BlockedByStatus) -> v1::BlockedByStatus {
    let (resolution, status, reason) = match value.resolution {
        task::BlockedByResolution::Found(status) => (
            v1::BlockedByResolutionKind::Found,
            Some(task_status(status) as i32),
            None,
        ),
        task::BlockedByResolution::Missing => (v1::BlockedByResolutionKind::Missing, None, None),
        task::BlockedByResolution::Unavailable { reason } => {
            (v1::BlockedByResolutionKind::Unavailable, None, Some(reason))
        }
    };
    v1::BlockedByStatus {
        id: value.id.to_string(),
        title: value.title,
        resolution: resolution as i32,
        status,
        reason,
    }
}

fn blocked_by_issue(value: task::BlockedByIssue) -> v1::BlockedByIssue {
    match value {
        task::BlockedByIssue::Malformed { path, raw, reason } => v1::BlockedByIssue {
            path: path.to_string(),
            raw,
            reason,
        },
    }
}

pub(crate) fn remove_confirmation(
    value: &confirmation::RemoveTaskConfirmation,
) -> v1::RemoveTaskConfirmation {
    v1::RemoveTaskConfirmation {
        task_id: value.task_identifier.to_string(),
        project: value.project.to_string(),
        title: value.title.to_string(),
        status: task_status(value.status) as i32,
        note_path: value.note_path.to_string(),
    }
}

pub(crate) fn reopen_confirmation(
    value: &confirmation::ReopenTaskConfirmation,
) -> v1::ReopenTaskConfirmation {
    v1::ReopenTaskConfirmation {
        task_id: value.task_identifier.to_string(),
        project: value.project.to_string(),
        completion_date: value.completion_date.map(|date| date.to_string()),
        commits: value.commit_provenance.clone(),
        report: value.report.clone(),
    }
}

pub(crate) fn dry_run_session(value: task::session::DryRunSession) -> v1::DryRunSession {
    v1::DryRunSession {
        plan: Some(session_plan(&value.plan)),
        argv: value.argv,
        probe: Some(agent_probe(&value.probe)),
        warnings: value.warnings.into_iter().map(session_warning).collect(),
    }
}

pub(crate) fn session_preflight(
    value: &task::session::PreparedSessionDispatch,
) -> v1::SessionDispatchPreflight {
    v1::SessionDispatchPreflight {
        confirmation: Some(dispatch_confirmation(&value.confirmation)),
        probe: Some(agent_probe(&value.probe)),
        warnings: value
            .warnings
            .clone()
            .into_iter()
            .map(session_warning)
            .collect(),
    }
}

pub(crate) fn dispatched_session(value: task::session::DispatchedSession) -> v1::DispatchedSession {
    match value {
        task::session::DispatchedSession::Aborted { task_id } => v1::DispatchedSession {
            outcome: v1::DispatchSessionOutcome::Aborted as i32,
            task_id: task_id.to_string(),
            inline_launch: None,
            window_opened: None,
        },
        task::session::DispatchedSession::InlineLaunch {
            task_id,
            argv,
            working_directory,
        } => v1::DispatchedSession {
            outcome: v1::DispatchSessionOutcome::InlineLaunch as i32,
            task_id: task_id.to_string(),
            inline_launch: Some(v1::InlineLaunch {
                task_id: task_id.to_string(),
                argv,
                working_directory: working_directory.to_string(),
            }),
            window_opened: None,
        },
        task::session::DispatchedSession::WindowOpened {
            target,
            agent,
            project_path,
        } => v1::DispatchedSession {
            outcome: v1::DispatchSessionOutcome::WindowOpened as i32,
            task_id: target.task_id().to_string(),
            inline_launch: None,
            window_opened: Some(v1::WindowOpened {
                task_id: target.task_id().to_string(),
                session: target.session_name(),
                agent: agent_value(agent) as i32,
                project_path: project_path.to_string(),
            }),
        },
    }
}

fn session_plan(value: &task::session::SessionPlan) -> v1::SessionPlan {
    v1::SessionPlan {
        launch: Some(v1::AgentLaunch {
            agent: agent_value(value.launch.agent) as i32,
            task_id: value.launch.task_id.to_string(),
            title: value.launch.title.to_string(),
            project_path: value.launch.project_path.to_string(),
            prompt: value.launch.prompt.to_string(),
            model: value.launch.model.as_deref().map(str::to_string),
            effort: session_effort(value.launch.effort) as i32,
        }),
        mode: dispatch_mode(value.mode) as i32,
    }
}

fn dispatch_confirmation(value: &task::session::DispatchConfirmation) -> v1::DispatchConfirmation {
    v1::DispatchConfirmation {
        task_id: value.task_id.to_string(),
        title: value.title.to_string(),
        created: value.created.as_ref().map(ToString::to_string),
        mode: dispatch_mode(value.mode) as i32,
        agent: agent_value(value.agent) as i32,
        directives: Some(v1::LaunchDirectives {
            worktree: value.directives.worktree,
            autonomous: value.directives.autonomous,
        }),
        has_pushed_prompt: value.has_pushed_prompt,
        model: value.model.as_deref().map(str::to_string),
        effort: session_effort(value.effort) as i32,
    }
}

fn agent_probe(value: &task::session::AgentProbe) -> v1::AgentProbe {
    let availability = match value.availability {
        task::session::AgentAvailability::Missing => v1::AgentAvailability::Missing,
        task::session::AgentAvailability::Available => v1::AgentAvailability::Available,
    };
    v1::AgentProbe {
        agent: agent_value(value.agent) as i32,
        availability: availability as i32,
    }
}

fn session_warning(value: task::session::SessionWarning) -> v1::SessionWarning {
    let value = match value {
        task::session::SessionWarning::BlockedBy(value) => {
            v1::session_warning::Value::BlockedBy(blocked_by_status(value))
        }
        task::session::SessionWarning::BlockedByMetadata(value) => {
            v1::session_warning::Value::BlockedByMetadata(blocked_by_issue(value))
        }
    };
    v1::SessionWarning { value: Some(value) }
}

fn task_status(value: TaskStatus) -> v1::TaskStatus {
    match value {
        TaskStatus::Active => v1::TaskStatus::Active,
        TaskStatus::Done => v1::TaskStatus::Done,
        TaskStatus::Cancelled => v1::TaskStatus::Cancelled,
    }
}

fn effort_tier(value: EffortTier) -> v1::EffortTier {
    match value {
        EffortTier::Low => v1::EffortTier::Low,
        EffortTier::Medium => v1::EffortTier::Medium,
        EffortTier::High => v1::EffortTier::High,
        EffortTier::Highest => v1::EffortTier::Highest,
    }
}

fn agent_value(value: Agent) -> v1::Agent {
    match value {
        Agent::Claude => v1::Agent::Claude,
        Agent::Codex => v1::Agent::Codex,
    }
}

fn dispatch_mode(value: DispatchMode) -> v1::DispatchMode {
    match value {
        DispatchMode::Inline => v1::DispatchMode::Inline,
        DispatchMode::Multiplexer => v1::DispatchMode::Multiplexer,
    }
}

fn session_effort(value: SessionEffort) -> v1::SessionEffort {
    match value {
        SessionEffort::Low => v1::SessionEffort::Low,
        SessionEffort::Medium => v1::SessionEffort::Medium,
        SessionEffort::High => v1::SessionEffort::High,
        SessionEffort::XHigh => v1::SessionEffort::Xhigh,
        SessionEffort::Max => v1::SessionEffort::Max,
    }
}

fn strings<T: ToString>(values: Vec<T>) -> Vec<String> {
    values.into_iter().map(|value| value.to_string()).collect()
}
