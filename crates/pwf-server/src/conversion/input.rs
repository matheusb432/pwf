use std::num::NonZeroUsize;

use pwf_application::contract::{note, project, task};
use pwf_models::{
    AppDate,
    note::{NoteDomain, NoteSelector, NoteTitle, NoteVerification, NoteWhy},
    project::{
        ProjectId, ProjectName, ProjectSelector, ProjectSource, ProjectSourceKind,
        ProjectSourceValue, ProjectTasks, ProjectTasksKind, ProjectTasksPath,
    },
    session::{AgentModel, LaunchDirectives, PushedPrompt},
    task::{BlockedBy, CommitRanges, Tag, TaskId, TaskPrompt, TaskReport, TaskTags, TaskTitle},
};
use pwf_wire::v1;
use tonic::Status;

use super::{invalid, parse, required};

pub(crate) fn add_project(request: v1::AddProjectRequest) -> Result<project::AddProject, Status> {
    Ok(project::AddProject {
        fields: project_fields(required("fields", request.fields)?)?,
    })
}

pub(crate) fn get_project(request: v1::GetProjectRequest) -> Result<project::GetProject, Status> {
    let v1::GetProjectRequest { id, status } = request;
    Ok(project::GetProject {
        id: parse("id", &id)?,
        status: project_status_filter(status)?,
    })
}

pub(crate) fn list_projects(
    request: v1::ListProjectsRequest,
) -> Result<project::ListProjects, Status> {
    Ok(project::ListProjects {
        status: project_status_filter(request.status)?,
    })
}

pub(crate) fn pause_project(
    request: v1::PauseProjectRequest,
) -> Result<project::PauseProject, Status> {
    let v1::PauseProjectRequest { id } = request;
    Ok(project::PauseProject {
        id: parse("id", &id)?,
    })
}

pub(crate) fn rename_project(
    request: v1::RenameProjectRequest,
) -> Result<project::RenameProject, Status> {
    Ok(project::RenameProject {
        current_id: parse("current_id", &request.current_id)?,
        fields: project_fields(required("fields", request.fields)?)?,
    })
}

pub(crate) fn resume_project(
    request: v1::ResumeProjectRequest,
) -> Result<project::ResumeProject, Status> {
    let v1::ResumeProjectRequest { id } = request;
    Ok(project::ResumeProject {
        id: parse("id", &id)?,
    })
}

fn project_fields(fields: v1::ProjectFields) -> Result<project::ProjectFields, Status> {
    let source_kind = ProjectSourceKind::try_from(fields.source_kind.as_str())
        .map_err(|error| invalid("fields.source_kind", error))?;
    let tasks_kind = ProjectTasksKind::try_from(fields.tasks_kind.as_str())
        .map_err(|error| invalid("fields.tasks_kind", error))?;
    Ok(project::ProjectFields {
        id: ProjectId::try_new(fields.id)
            .map_err(|_| invalid("fields.id", "expected two to four ASCII letters"))?,
        title: ProjectName::try_new(fields.title)
            .map_err(|error| invalid("fields.title", error))?,
        source: ProjectSource::new(
            source_kind,
            ProjectSourceValue::try_new(fields.source_value)
                .map_err(|_| invalid("fields.source_value", "must not be blank"))?,
        ),
        tasks: ProjectTasks::new(
            tasks_kind,
            ProjectTasksPath::try_new(fields.tasks_path)
                .map_err(|_| invalid("fields.tasks_path", "must not be blank"))?,
        ),
    })
}

fn project_status_filter(value: i32) -> Result<project::ProjectStatusFilter, Status> {
    match v1::ProjectStatusFilter::try_from(value).ok() {
        Some(v1::ProjectStatusFilter::ActiveOnly) => Ok(project::ProjectStatusFilter::ActiveOnly),
        Some(v1::ProjectStatusFilter::IncludingPaused) => {
            Ok(project::ProjectStatusFilter::IncludingPaused)
        }
        Some(v1::ProjectStatusFilter::Unspecified) | None => {
            Err(invalid("status", "must be specified"))
        }
    }
}

pub(crate) fn add_note(request: v1::AddNoteRequest) -> Result<note::AddNote, Status> {
    Ok(note::AddNote {
        project_selector: parse("project_selector", &request.project_selector)?,
        title: parse("title", &request.title)?,
        content: parse("content", &request.content)?,
        why: request
            .why
            .as_deref()
            .map(|value| parse::<NoteWhy>("why", value))
            .transpose()?,
        domain: request
            .domain
            .as_deref()
            .map(|value| parse::<NoteDomain>("domain", value))
            .transpose()?,
        tags: parse_repeated("tags", request.tags)?,
        sources: parse_repeated("sources", request.sources)?,
        verified: request
            .verified
            .as_deref()
            .map(|value| parse::<NoteVerification>("verified", value))
            .transpose()?,
        date: request
            .date
            .as_deref()
            .map(|value| parse::<AppDate>("date", value))
            .transpose()?,
    })
}

pub(crate) fn list_notes(request: v1::ListNotesRequest) -> Result<note::ListNotes, Status> {
    let v1::ListNotesRequest {
        project_selector,
        limit_kind,
        limit,
    } = request;
    let limit = match v1::NoteListLimitKind::try_from(limit_kind).ok() {
        Some(v1::NoteListLimitKind::Default) => note::NoteListLimit::Default,
        Some(v1::NoteListLimitKind::Unlimited) => note::NoteListLimit::Unlimited,
        Some(v1::NoteListLimitKind::AtMost) => {
            let value = usize::try_from(limit)
                .ok()
                .and_then(NonZeroUsize::new)
                .ok_or_else(|| invalid("limit", "must be a positive platform-sized integer"))?;
            note::NoteListLimit::AtMost(value)
        }
        Some(v1::NoteListLimitKind::Unspecified) | None => {
            return Err(invalid("limit_kind", "must be specified"));
        }
    };
    Ok(note::ListNotes {
        project_selector: parse("project_selector", &project_selector)?,
        limit,
    })
}

pub(crate) fn remove_note(request: v1::RemoveNoteRequest) -> Result<note::RemoveNote, Status> {
    let v1::RemoveNoteRequest {
        project_selector,
        selector,
    } = request;
    Ok(note::RemoveNote {
        project_selector: parse("project_selector", &project_selector)?,
        selector: parse::<NoteSelector>("selector", &selector)?,
    })
}

pub(crate) fn update_note(request: v1::UpdateNoteRequest) -> Result<note::UpdateNote, Status> {
    let v1::UpdateNoteRequest {
        project_selector,
        selector,
        title,
    } = request;
    Ok(note::UpdateNote {
        project_selector: parse("project_selector", &project_selector)?,
        selector: parse::<NoteSelector>("selector", &selector)?,
        title: parse::<NoteTitle>("title", &title)?,
    })
}

pub(crate) fn add_task(request: v1::AddTaskRequest) -> Result<task::AddTask, Status> {
    let prompt = match required("prompt", request.prompt)? {
        v1::add_task_request::Prompt::Shorthand(value) => {
            task::AddTaskPrompt::shorthand(TaskPrompt::new(value))
                .map_err(|error| invalid("prompt", error))?
        }
        v1::add_task_request::Prompt::Structured(value) => {
            let title =
                TaskTitle::try_new(value.title).map_err(|error| invalid("prompt.title", error))?;
            task::AddTaskPrompt::structured(title, task_lanes(value.lanes.unwrap_or_default())?)
        }
    };
    Ok(task::AddTask {
        project_selector: parse::<ProjectSelector>("project_selector", &request.project_selector)?,
        prompt,
        index_section: match v1::IndexSection::try_from(request.index_section).ok() {
            Some(v1::IndexSection::General) => pwf_models::task::IndexSection::General,
            Some(v1::IndexSection::Human) => pwf_models::task::IndexSection::Human,
            Some(v1::IndexSection::Unspecified) | None => {
                return Err(invalid("index_section", "must be specified"));
            }
        },
        blocked_by: blocked_by(request.blocked_by)?,
        effort: request.effort.map(effort).transpose()?,
        tags: task_tags(request.tags)?,
    })
}

pub(crate) fn cancel_task(request: v1::CancelTaskRequest) -> Result<task::CancelTask, Status> {
    let v1::CancelTaskRequest {
        id,
        report,
        commits,
        review,
    } = request;
    Ok(task::CancelTask {
        id: parse::<TaskId>("id", &id)?,
        report: parse::<TaskReport>("report", &report)?,
        commits: CommitRanges::from_inputs(&commits),
        review,
    })
}

pub(crate) fn complete_task(
    request: v1::CompleteTaskRequest,
) -> Result<task::CompleteTask, Status> {
    let v1::CompleteTaskRequest {
        id,
        report,
        commits,
        review,
    } = request;
    Ok(task::CompleteTask {
        id: parse::<TaskId>("id", &id)?,
        report: report
            .as_deref()
            .map(|value| parse::<TaskReport>("report", value))
            .transpose()?,
        commits: CommitRanges::from_inputs(&commits),
        review,
    })
}

pub(crate) fn edit_task(request: v1::EditTaskRequest) -> Result<task::EditTask, Status> {
    let content = request.content.map(task_content_edit).transpose()?;
    let edits = task::TaskEdits::try_new(
        content,
        collection_edit(request.blocked_by, blocked_by_values)?,
        effort_edit(request.effort)?,
        collection_edit(request.tags, task_tag_values)?,
    )
    .map_err(|error| invalid("edits", error))?;
    Ok(task::EditTask {
        id: parse::<TaskId>("id", &request.id)?,
        edits,
    })
}

fn task_content_edit(value: v1::TaskContentEdit) -> Result<task::EditTaskContent, Status> {
    match required("content", value.content)? {
        v1::task_content_edit::Content::Structured(value) => {
            let title = value
                .title
                .map(TaskTitle::try_new)
                .transpose()
                .map_err(|error| invalid("content.title", error))?;
            let additions = task_lanes(value.additions.unwrap_or_default())?;
            let removals = value
                .removals
                .into_iter()
                .map(task_lane)
                .collect::<Result<Vec<_>, _>>()?;
            task::EditTaskContent::structured(title, task::TaskLaneEdits::new(additions, removals))
                .map_err(|error| invalid("content", error))
        }
        v1::task_content_edit::Content::Append(value) => {
            let title = value
                .title
                .map(TaskTitle::try_new)
                .transpose()
                .map_err(|error| invalid("content.title", error))?;
            task::EditTaskContent::append_shorthand(title, TaskPrompt::new(value.prompt))
                .map_err(|error| invalid("content", error))
        }
        v1::task_content_edit::Content::Replace(value) => {
            task::EditTaskContent::replace_shorthand(TaskPrompt::new(value))
                .map_err(|error| invalid("content", error))
        }
    }
}

fn collection_edit<T>(
    value: Option<v1::StringCollectionEdit>,
    parse_values: fn(Vec<String>) -> Result<Option<T>, Status>,
) -> Result<task::CollectionEdit<T>, Status> {
    let Some(value) = value else {
        return Ok(task::CollectionEdit::Unchanged);
    };
    let mode = v1::CollectionEditMode::try_from(value.mode)
        .map_err(|_| invalid("collection_edit.mode", "unknown value"))?;
    match mode {
        v1::CollectionEditMode::Unchanged if value.values.is_empty() => {
            Ok(task::CollectionEdit::Unchanged)
        }
        v1::CollectionEditMode::Clear if value.values.is_empty() => Ok(task::CollectionEdit::Clear),
        v1::CollectionEditMode::Append | v1::CollectionEditMode::Replace => {
            let parsed = parse_values(value.values)?
                .ok_or_else(|| invalid("collection_edit.values", "cannot be empty"))?;
            Ok(if mode == v1::CollectionEditMode::Append {
                task::CollectionEdit::Append(parsed)
            } else {
                task::CollectionEdit::Replace(parsed)
            })
        }
        v1::CollectionEditMode::Unspecified => {
            Err(invalid("collection_edit.mode", "must be specified"))
        }
        v1::CollectionEditMode::Unchanged | v1::CollectionEditMode::Clear => Err(invalid(
            "collection_edit.values",
            "must be empty for this mode",
        )),
    }
}

fn effort_edit(
    value: Option<v1::EffortEdit>,
) -> Result<task::ValueEdit<pwf_models::task::EffortTier>, Status> {
    let Some(value) = value else {
        return Ok(task::ValueEdit::Unchanged);
    };
    match v1::ValueEditMode::try_from(value.mode).ok() {
        Some(v1::ValueEditMode::Unchanged) => Ok(task::ValueEdit::Unchanged),
        Some(v1::ValueEditMode::Clear) => Ok(task::ValueEdit::Clear),
        Some(v1::ValueEditMode::Set) => Ok(task::ValueEdit::Set(effort(value.value)?)),
        Some(v1::ValueEditMode::Unspecified) | None => {
            Err(invalid("effort.mode", "must be specified"))
        }
    }
}

pub(crate) fn get_task(request: v1::GetTaskRequest) -> Result<task::GetTask, Status> {
    let v1::GetTaskRequest { id, output } = request;
    let output = match v1::TaskReadFormat::try_from(output).ok() {
        Some(v1::TaskReadFormat::Markdown) => task::TaskReadFormat::Markdown,
        Some(v1::TaskReadFormat::Path) => task::TaskReadFormat::Path,
        Some(v1::TaskReadFormat::Data) => task::TaskReadFormat::Data,
        Some(v1::TaskReadFormat::Unspecified) | None => {
            return Err(invalid("output", "must be specified"));
        }
    };
    Ok(task::GetTask {
        id: parse::<TaskId>("id", &id)?,
        output,
    })
}

pub(crate) fn list_tasks(request: v1::ListTasksRequest) -> Result<task::ListTasks, Status> {
    let scope = match v1::ListScope::try_from(request.scope).ok() {
        Some(v1::ListScope::Default) => task::ListScope::Default,
        Some(v1::ListScope::Human) => task::ListScope::Human,
        Some(v1::ListScope::Future) => task::ListScope::Future,
        Some(v1::ListScope::All) => task::ListScope::All,
        Some(v1::ListScope::Unspecified) | None => {
            return Err(invalid("scope", "must be specified"));
        }
    };
    let detail = match v1::ListDetail::try_from(request.detail).ok() {
        Some(v1::ListDetail::Summary) => task::ListDetail::Summary,
        Some(v1::ListDetail::Detailed) => task::ListDetail::Detailed,
        Some(v1::ListDetail::Unspecified) | None => {
            return Err(invalid("detail", "must be specified"));
        }
    };
    Ok(task::ListTasks {
        project_selector: request
            .project_selector
            .as_deref()
            .map(|value| parse::<ProjectSelector>("project_selector", value))
            .transpose()?,
        scope,
        number: request
            .number
            .map(|value| {
                usize::try_from(value)
                    .ok()
                    .and_then(NonZeroUsize::new)
                    .ok_or_else(|| invalid("number", "must be a positive platform-sized integer"))
            })
            .transpose()?,
        effort: request.effort.map(effort).transpose()?,
        tags: task_tags(request.tags)?,
        order: request.order.map(order_spec).transpose()?,
        status: request.status.map(task_status_filter).transpose()?,
        detail,
    })
}

pub(crate) fn remove_task(request: v1::RemoveTaskRequest) -> Result<task::RemoveTask, Status> {
    let v1::RemoveTaskRequest { id } = request;
    Ok(task::RemoveTask {
        id: parse::<TaskId>("id", &id)?,
    })
}

pub(crate) fn reopen_task(request: v1::ReopenTaskRequest) -> Result<task::ReopenTask, Status> {
    let v1::ReopenTaskRequest { id } = request;
    Ok(task::ReopenTask {
        id: parse::<TaskId>("id", &id)?,
    })
}

pub(crate) fn plan_session(
    request: v1::PlanSessionRequest,
) -> Result<task::session::PlanSession, Status> {
    let intent = match v1::PlanSessionIntent::try_from(request.intent).ok() {
        Some(v1::PlanSessionIntent::DryRun) => task::session::PlanSessionIntent::DryRun,
        Some(v1::PlanSessionIntent::Dispatch) => task::session::PlanSessionIntent::Dispatch,
        Some(v1::PlanSessionIntent::Unspecified) | None => {
            return Err(invalid("intent", "must be specified"));
        }
    };
    let mode = match v1::DispatchMode::try_from(request.mode).ok() {
        Some(v1::DispatchMode::Inline) => pwf_models::session::DispatchMode::Inline,
        Some(v1::DispatchMode::Multiplexer) => pwf_models::session::DispatchMode::Multiplexer,
        Some(v1::DispatchMode::Unspecified) | None => {
            return Err(invalid("mode", "must be specified"));
        }
    };
    let agent = match v1::Agent::try_from(request.agent).ok() {
        Some(v1::Agent::Claude) => pwf_models::session::Agent::Claude,
        Some(v1::Agent::Codex) => pwf_models::session::Agent::Codex,
        Some(v1::Agent::Unspecified) | None => return Err(invalid("agent", "must be specified")),
    };
    let effort = match v1::SessionEffort::try_from(request.effort).ok() {
        Some(v1::SessionEffort::Low) => pwf_models::session::SessionEffort::Low,
        Some(v1::SessionEffort::Medium) => pwf_models::session::SessionEffort::Medium,
        Some(v1::SessionEffort::High) => pwf_models::session::SessionEffort::High,
        Some(v1::SessionEffort::Xhigh) => pwf_models::session::SessionEffort::XHigh,
        Some(v1::SessionEffort::Max) => pwf_models::session::SessionEffort::Max,
        Some(v1::SessionEffort::Unspecified) | None => {
            return Err(invalid("effort", "must be specified"));
        }
    };
    let directives = request.directives.unwrap_or_default();
    Ok(task::session::PlanSession {
        task_id: parse::<TaskId>("task_id", &request.task_id)?,
        intent,
        pushed_prompt: request
            .pushed_prompt
            .map(PushedPrompt::try_new)
            .transpose()
            .map_err(|error| invalid("pushed_prompt", error))?,
        mode,
        directives: LaunchDirectives {
            worktree: directives.worktree,
            autonomous: directives.autonomous,
        },
        agent,
        model_override: AgentModel::from(request.model_override),
        effort,
    })
}

fn parse_repeated<T>(field: &str, values: Vec<String>) -> Result<Vec<T>, Status>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| parse(&format!("{field}[{index}]"), &value))
        .collect()
}

fn task_lanes(value: v1::TaskLanes) -> Result<task::TaskLanes, Status> {
    task::TaskLanes::try_new(
        value.goals,
        value.context,
        value.constraints,
        value.done_when,
    )
    .map_err(|error| invalid("lanes", error))
}

fn task_lane(value: i32) -> Result<task::TaskLane, Status> {
    match v1::TaskLane::try_from(value).ok() {
        Some(v1::TaskLane::Goal) => Ok(task::TaskLane::Goal),
        Some(v1::TaskLane::Context) => Ok(task::TaskLane::Context),
        Some(v1::TaskLane::Constraint) => Ok(task::TaskLane::Constraint),
        Some(v1::TaskLane::DoneWhen) => Ok(task::TaskLane::DoneWhen),
        Some(v1::TaskLane::Unspecified) | None => Err(invalid("lane", "must be specified")),
    }
}

fn effort(value: i32) -> Result<pwf_models::task::EffortTier, Status> {
    match v1::EffortTier::try_from(value).ok() {
        Some(v1::EffortTier::Low) => Ok(pwf_models::task::EffortTier::Low),
        Some(v1::EffortTier::Medium) => Ok(pwf_models::task::EffortTier::Medium),
        Some(v1::EffortTier::High) => Ok(pwf_models::task::EffortTier::High),
        Some(v1::EffortTier::Highest) => Ok(pwf_models::task::EffortTier::Highest),
        Some(v1::EffortTier::Unspecified) | None => Err(invalid("effort", "must be specified")),
    }
}

fn blocked_by(values: Vec<String>) -> Result<Option<BlockedBy>, Status> {
    blocked_by_values(values)
}

fn blocked_by_values(values: Vec<String>) -> Result<Option<BlockedBy>, Status> {
    if values.is_empty() {
        return Ok(None);
    }
    let values = values
        .into_iter()
        .enumerate()
        .map(|(index, value)| parse::<TaskId>(&format!("blocked_by[{index}]"), &value))
        .collect::<Result<Vec<_>, _>>()?;
    BlockedBy::try_new(values)
        .map(Some)
        .map_err(|error| invalid("blocked_by", error))
}

fn task_tags(values: Vec<String>) -> Result<Option<TaskTags>, Status> {
    task_tag_values(values)
}

fn task_tag_values(values: Vec<String>) -> Result<Option<TaskTags>, Status> {
    if values.is_empty() {
        return Ok(None);
    }
    let values = values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            Tag::try_from(value.as_str()).map_err(|error| invalid(&format!("tags[{index}]"), error))
        })
        .collect::<Result<Vec<_>, _>>()?;
    TaskTags::try_new(values)
        .map(Some)
        .map_err(|error| invalid("tags", error))
}

fn order_spec(value: v1::OrderSpec) -> Result<task::OrderSpec, Status> {
    let field = match v1::OrderField::try_from(value.field).ok() {
        Some(v1::OrderField::Created) => task::OrderField::Created,
        Some(v1::OrderField::Id) => task::OrderField::Id,
        Some(v1::OrderField::ProjectId) => task::OrderField::ProjectId,
        Some(v1::OrderField::Unspecified) | None => {
            return Err(invalid("order.field", "must be specified"));
        }
    };
    let direction = match v1::OrderDirection::try_from(value.direction).ok() {
        Some(v1::OrderDirection::Asc) => task::OrderDirection::Asc,
        Some(v1::OrderDirection::Desc) => task::OrderDirection::Desc,
        Some(v1::OrderDirection::Unspecified) | None => {
            return Err(invalid("order.direction", "must be specified"));
        }
    };
    Ok(task::OrderSpec { field, direction })
}

fn task_status_filter(value: i32) -> Result<task::StatusFilter, Status> {
    match v1::TaskStatusFilter::try_from(value).ok() {
        Some(v1::TaskStatusFilter::Active) => Ok(task::StatusFilter::Exact(
            pwf_models::task::TaskStatus::Active,
        )),
        Some(v1::TaskStatusFilter::Done) => Ok(task::StatusFilter::Exact(
            pwf_models::task::TaskStatus::Done,
        )),
        Some(v1::TaskStatusFilter::Cancelled) => Ok(task::StatusFilter::Exact(
            pwf_models::task::TaskStatus::Cancelled,
        )),
        Some(v1::TaskStatusFilter::All) => Ok(task::StatusFilter::All),
        Some(v1::TaskStatusFilter::Unspecified) | None => {
            Err(invalid("status", "must be specified"))
        }
    }
}
