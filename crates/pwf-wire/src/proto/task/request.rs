//! Protobuf request mappings for task operations.

use pwf_models::{
    project::ProjectSelector,
    task::{
        BlockedBy, CommitRanges, EffortTier, IndexSection, PriorityTier, Tag, TaskId, TaskPrompt,
        TaskReport, TaskStatus, TaskTags, TaskTitle,
    },
};
use tonic::Status;

use super::super::{invalid, parse, required};
use crate::{task, v1};

pub fn create_task_request(request: v1::CreateTaskRequest) -> Result<task::AddTask, Status> {
    let prompt = match required("prompt", request.prompt)? {
        v1::create_task_request::Prompt::Shorthand(value) => {
            task::AddTaskPrompt::shorthand(TaskPrompt::new(value))
                .map_err(|error| invalid("prompt", error))?
        }
        v1::create_task_request::Prompt::Structured(value) => {
            let title =
                TaskTitle::try_new(value.title).map_err(|error| invalid("prompt.title", error))?;
            task::AddTaskPrompt::structured(title, task_lanes(value.lanes.unwrap_or_default())?)
        }
    };
    Ok(task::AddTask {
        project_selector: parse::<ProjectSelector>("project_selector", &request.project_selector)?,
        prompt,
        index_section: match v1::IndexSection::try_from(request.index_section).ok() {
            Some(v1::IndexSection::General) => IndexSection::General,
            Some(v1::IndexSection::Human) => IndexSection::Human,
            Some(v1::IndexSection::Unspecified) | None => {
                return Err(invalid("index_section", "must be specified"));
            }
        },
        blocked_by: blocked_by_values(request.blocked_by)?,
        effort: request.effort.map(effort_tier).transpose()?,
        tags: task_tag_values(request.tags)?,
        priority: request.priority.map(priority_tier).transpose()?,
    })
}

pub fn cancel_task_request(request: v1::CancelTaskRequest) -> Result<task::CancelTask, Status> {
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

pub fn complete_task_request(
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

pub fn update_task_request(request: v1::UpdateTaskRequest) -> Result<task::EditTask, Status> {
    let content = request.content.map(task_content_edit).transpose()?;
    let edits = task::TaskEdits::try_new(
        content,
        collection_edit(request.blocked_by, blocked_by_values)?,
        effort_edit(request.effort)?,
        collection_edit(request.tags, task_tag_values)?,
        priority_edit(request.priority)?,
    )
    .map_err(|error| invalid("edits", error))?;
    Ok(task::EditTask {
        id: parse::<TaskId>("id", &request.id)?,
        edits,
    })
}

pub fn get_task_request(request: v1::GetTaskRequest) -> Result<task::GetTask, Status> {
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

pub fn list_tasks_request(request: v1::ListTasksRequest) -> Result<task::ListTasks, Status> {
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
                    .map_err(|_| invalid("number", "must fit the platform integer size"))
                    .and_then(|value| {
                        task::TaskListLimit::try_new(value)
                            .map_err(|error| invalid("number", error))
                    })
            })
            .transpose()?,
        effort: request.effort.map(effort_tier).transpose()?,
        priority: request.priority.map(priority_tier).transpose()?,
        tags: task_tag_values(request.tags)?,
        order: request.order.map(order_spec).transpose()?,
        status: request.status.map(status_filter).transpose()?,
        detail,
    })
}

pub fn delete_task_start(start: v1::DeleteTaskStart) -> Result<TaskId, Status> {
    let v1::DeleteTaskStart { id } = start;
    parse("id", &id)
}

pub fn reopen_task_start(start: v1::ReopenTaskStart) -> Result<TaskId, Status> {
    let v1::ReopenTaskStart { id } = start;
    parse("id", &id)
}

fn task_content_edit(edit: v1::TaskContentEdit) -> Result<task::EditTaskContent, Status> {
    match required("content", edit.content)? {
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
                .collect::<Result<Vec<task::TaskLane>, _>>()?;
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

fn task_lanes(lanes: v1::TaskLanes) -> Result<task::TaskLanes, Status> {
    task::TaskLanes::try_new(
        lanes.goals,
        lanes.context,
        lanes.constraints,
        lanes.done_when,
    )
    .map_err(|error| invalid("lanes", error))
}

fn effort_tier(value: i32) -> Result<EffortTier, Status> {
    match v1::EffortTier::try_from(value).ok() {
        Some(v1::EffortTier::Low) => Ok(EffortTier::Low),
        Some(v1::EffortTier::Medium) => Ok(EffortTier::Medium),
        Some(v1::EffortTier::High) => Ok(EffortTier::High),
        Some(v1::EffortTier::Highest) => Ok(EffortTier::Highest),
        Some(v1::EffortTier::Unspecified) | None => Err(invalid("effort", "must be specified")),
    }
}

fn priority_tier(value: i32) -> Result<PriorityTier, Status> {
    match v1::PriorityTier::try_from(value).ok() {
        Some(v1::PriorityTier::Low) => Ok(PriorityTier::Low),
        Some(v1::PriorityTier::Medium) => Ok(PriorityTier::Medium),
        Some(v1::PriorityTier::High) => Ok(PriorityTier::High),
        Some(v1::PriorityTier::Highest) => Ok(PriorityTier::Highest),
        Some(v1::PriorityTier::Unspecified) | None => Err(invalid("priority", "must be specified")),
    }
}

fn order_spec(order: v1::OrderSpec) -> Result<task::OrderSpec, Status> {
    Ok(task::OrderSpec {
        field: match v1::OrderField::try_from(order.field).ok() {
            Some(v1::OrderField::Created) => task::OrderField::Created,
            Some(v1::OrderField::Id) => task::OrderField::Id,
            Some(v1::OrderField::ProjectId) => task::OrderField::ProjectId,
            Some(v1::OrderField::Unspecified) | None => {
                return Err(invalid("order.field", "must be specified"));
            }
        },
        direction: match v1::OrderDirection::try_from(order.direction).ok() {
            Some(v1::OrderDirection::Asc) => task::OrderDirection::Asc,
            Some(v1::OrderDirection::Desc) => task::OrderDirection::Desc,
            Some(v1::OrderDirection::Unspecified) | None => {
                return Err(invalid("order.direction", "must be specified"));
            }
        },
    })
}

fn status_filter(value: i32) -> Result<task::StatusFilter, Status> {
    match v1::TaskStatusFilter::try_from(value).ok() {
        Some(v1::TaskStatusFilter::Active) => Ok(task::StatusFilter::Exact(TaskStatus::Active)),
        Some(v1::TaskStatusFilter::Done) => Ok(task::StatusFilter::Exact(TaskStatus::Done)),
        Some(v1::TaskStatusFilter::Cancelled) => {
            Ok(task::StatusFilter::Exact(TaskStatus::Cancelled))
        }
        Some(v1::TaskStatusFilter::All) => Ok(task::StatusFilter::All),
        Some(v1::TaskStatusFilter::Unspecified) | None => {
            Err(invalid("status", "must be specified"))
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
    match required("collection_edit.operation", value.operation)? {
        v1::string_collection_edit::Operation::Append(values) => parse_values(values.values)?
            .map(task::CollectionEdit::Append)
            .ok_or_else(|| invalid("collection_edit.values", "cannot be empty")),
        v1::string_collection_edit::Operation::Replace(values) => parse_values(values.values)?
            .map(task::CollectionEdit::Replace)
            .ok_or_else(|| invalid("collection_edit.values", "cannot be empty")),
        v1::string_collection_edit::Operation::Clear(_) => Ok(task::CollectionEdit::Clear),
    }
}

fn effort_edit(value: Option<v1::EffortEdit>) -> Result<task::ValueEdit<EffortTier>, Status> {
    let Some(value) = value else {
        return Ok(task::ValueEdit::Unchanged);
    };
    match required("effort.operation", value.operation)? {
        v1::effort_edit::Operation::Set(value) => Ok(task::ValueEdit::Set(effort_tier(value)?)),
        v1::effort_edit::Operation::Clear(_) => Ok(task::ValueEdit::Clear),
    }
}

fn priority_edit(value: Option<v1::PriorityEdit>) -> Result<task::ValueEdit<PriorityTier>, Status> {
    let Some(value) = value else {
        return Ok(task::ValueEdit::Unchanged);
    };
    match required("priority.operation", value.operation)? {
        v1::priority_edit::Operation::Set(value) => Ok(task::ValueEdit::Set(priority_tier(value)?)),
        v1::priority_edit::Operation::Clear(_) => Ok(task::ValueEdit::Clear),
    }
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
