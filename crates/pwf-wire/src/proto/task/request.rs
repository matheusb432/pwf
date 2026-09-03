//! Protobuf request mappings for task operations.

use prost::Message;
use pwf_models::{
    project::ProjectSelector,
    revision::ContentRevision,
    task::{
        BlockedBy, CommitRanges, EffortTier, PriorityTier, Tag, TaskId, TaskPrompt, TaskReport,
        TaskSection, TaskStatus, TaskTags, TaskTitle,
    },
};
use tonic::Status;

use super::super::{collection_edit, invalid, parse, required};
use crate::{field_update::FieldUpdate, pb, task};

const TASK_COLLECTION_VALUES_MAX: usize = 64;
const TASK_LANE_VALUES_MAX: usize = 128;

pub fn create_task_request(request: pb::CreateTaskRequest) -> Result<task::AddTask, Status> {
    ensure_count(
        "blocked_by",
        request.blocked_by.len(),
        TASK_COLLECTION_VALUES_MAX,
    )?;
    ensure_count("tags", request.tags.len(), TASK_COLLECTION_VALUES_MAX)?;
    if let Some(pb::create_task_request::Prompt::Structured(prompt)) = request.prompt.as_ref() {
        ensure_lanes("prompt.lanes", prompt.lanes.as_ref(), 0)?;
    }
    let request_fingerprint = request_fingerprint(&request, |request| {
        request.request_id.clear();
    });
    let pb::CreateTaskRequest {
        project_selector,
        prompt,
        blocked_by,
        effort,
        tags,
        priority,
        request_id: request_id_value,
    } = request;
    let request_id = request_id(request_id_value)?;
    let prompt = match required("prompt", prompt)? {
        pb::create_task_request::Prompt::Shorthand(value) => {
            task::AddTaskPrompt::shorthand(TaskPrompt::new(value))
                .map_err(|error| invalid("prompt", error))?
        }
        pb::create_task_request::Prompt::Structured(value) => {
            let title =
                TaskTitle::try_new(value.title).map_err(|error| invalid("prompt.title", error))?;
            task::AddTaskPrompt::structured(title, task_lanes(value.lanes.unwrap_or_default())?)
        }
    };
    Ok(task::AddTask {
        project_selector: parse::<ProjectSelector>("project_selector", &project_selector)?,
        prompt,
        blocked_by: blocked_by_values(blocked_by)?,
        effort: effort.map(effort_tier).transpose()?,
        tags: task_tag_values(tags)?,
        priority: priority.map(priority_tier).transpose()?,
        request_id: Some(request_id),
        request_fingerprint: Some(request_fingerprint),
    })
}

pub fn cancel_task_request(request: pb::CancelTaskRequest) -> Result<task::CancelTask, Status> {
    ensure_count("commits", request.commits.len(), TASK_COLLECTION_VALUES_MAX)?;
    let request_fingerprint = request_fingerprint(&request, |request| {
        request.request_id.clear();
    });
    let pb::CancelTaskRequest {
        id,
        report,
        commits,
        expected_revision,
        request_id: request_id_value,
    } = request;
    Ok(task::CancelTask {
        id: parse::<TaskId>("id", &id)?,
        report: parse::<TaskReport>("report", &report)?,
        commits: CommitRanges::from_inputs(&commits),
        expected_revision: expected_revision.map(revision).transpose()?,
        request_id: Some(request_id(request_id_value)?),
        request_fingerprint: Some(request_fingerprint),
    })
}

pub fn complete_task_request(
    request: pb::CompleteTaskRequest,
) -> Result<task::CompleteTask, Status> {
    ensure_count("commits", request.commits.len(), TASK_COLLECTION_VALUES_MAX)?;
    let request_fingerprint = request_fingerprint(&request, |request| {
        request.request_id.clear();
    });
    let pb::CompleteTaskRequest {
        id,
        report,
        commits,
        expected_revision,
        request_id: request_id_value,
    } = request;
    Ok(task::CompleteTask {
        id: parse::<TaskId>("id", &id)?,
        report: report
            .as_deref()
            .map(|value| parse::<TaskReport>("report", value))
            .transpose()?,
        commits: CommitRanges::from_inputs(&commits),
        expected_revision: expected_revision.map(revision).transpose()?,
        request_id: Some(request_id(request_id_value)?),
        request_fingerprint: Some(request_fingerprint),
    })
}

pub fn update_task_request(request: pb::UpdateTaskRequest) -> Result<task::EditTask, Status> {
    ensure_update_bounds(&request)?;
    let request_fingerprint = request_fingerprint(&request, |request| {
        request.request_id.clear();
    });
    let pb::UpdateTaskRequest {
        id,
        content,
        blocked_by,
        effort,
        tags,
        priority,
        expected_revision,
        request_id: request_id_value,
    } = request;
    let content = content.map(task_content_edit).transpose()?;
    let edits = task::TaskEdits::try_new(
        content,
        collection_edit(blocked_by, blocked_by_values)?,
        effort_edit(effort)?,
        collection_edit(tags, task_tag_values)?,
        priority_edit(priority)?,
    )
    .map_err(|error| invalid("edits", error))?;
    Ok(task::EditTask {
        id: parse::<TaskId>("id", &id)?,
        edits,
        expected_revision: expected_revision.map(revision).transpose()?,
        request_id: Some(request_id(request_id_value)?),
        request_fingerprint: Some(request_fingerprint),
    })
}

pub fn get_task_request(request: pb::GetTaskRequest) -> Result<task::GetTask, Status> {
    let pb::GetTaskRequest { id, output } = request;
    let output = match pb::TaskReadFormat::try_from(output).ok() {
        Some(pb::TaskReadFormat::Markdown) => task::TaskReadFormat::Markdown,
        Some(pb::TaskReadFormat::Path) => task::TaskReadFormat::Path,
        Some(pb::TaskReadFormat::Data) => task::TaskReadFormat::Data,
        Some(pb::TaskReadFormat::Unspecified) | None => {
            return Err(invalid("output", "must be specified"));
        }
    };
    Ok(task::GetTask {
        id: parse::<TaskId>("id", &id)?,
        output,
    })
}

pub fn get_task_dag_request(request: pb::GetTaskDagRequest) -> Result<task::GetTaskDag, Status> {
    let pb::GetTaskDagRequest {
        id,
        depth,
        status,
        mode,
    } = request;
    let mode = match pb::TaskDagMode::try_from(mode).ok() {
        Some(pb::TaskDagMode::BlockedBy) => task::TaskDagMode::BlockedBy,
        Some(pb::TaskDagMode::Blocks) => task::TaskDagMode::Blocks,
        Some(pb::TaskDagMode::Full) => task::TaskDagMode::Full,
        Some(pb::TaskDagMode::Unspecified) | None => {
            return Err(invalid("mode", "must be specified"));
        }
    };
    Ok(task::GetTaskDag {
        id: parse::<TaskId>("id", &id)?,
        depth: depth
            .map(task::TaskDagDepth::try_new)
            .transpose()
            .map_err(|error| invalid("depth", error))?,
        status: status_filter(status)?,
        mode,
    })
}

pub fn list_tasks_request(request: pb::ListTasksRequest) -> Result<task::ListTasks, Status> {
    ensure_count("tags", request.tags.len(), TASK_COLLECTION_VALUES_MAX)?;
    let page_size = if request.page_size == 0 {
        task::TaskPageSize::DEFAULT
    } else {
        usize::try_from(request.page_size)
            .map_err(|_| invalid("page_size", "must fit the platform integer size"))?
    };
    let page_size =
        task::TaskPageSize::try_new(page_size).map_err(|error| invalid("page_size", error))?;
    let page_token = request
        .page_token
        .as_deref()
        .map(task::TaskPageToken::try_new)
        .transpose()
        .map_err(|error| invalid("page_token", error))?;
    let scope = match request.scope {
        Some(pb::list_tasks_request::Scope::Section(section)) => task::ListScope::Section(
            TaskSection::try_new(section).map_err(|error| invalid("section", error))?,
        ),
        Some(pb::list_tasks_request::Scope::All(_)) => task::ListScope::All,
        None => task::ListScope::Default,
    };
    let detail = match pb::ListDetail::try_from(request.detail).ok() {
        Some(pb::ListDetail::Summary) => task::ListDetail::Summary,
        Some(pb::ListDetail::Detailed) => task::ListDetail::Detailed,
        Some(pb::ListDetail::Unspecified) | None => {
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
        page_size: Some(page_size),
        page_token,
    })
}

pub fn delete_task_start(start: pb::DeleteTaskStart) -> Result<task::DeleteTask, Status> {
    let request_fingerprint = request_fingerprint(&start, |start| {
        start.request_id.clear();
    });
    let pb::DeleteTaskStart {
        id,
        request_id: request_id_value,
    } = start;
    Ok(task::DeleteTask {
        id: parse("id", &id)?,
        request_id: Some(request_id(request_id_value)?),
        request_fingerprint: Some(request_fingerprint),
    })
}

pub fn reopen_task_start(start: pb::ReopenTaskStart) -> Result<task::ReopenTask, Status> {
    let request_fingerprint = request_fingerprint(&start, |start| {
        start.request_id.clear();
    });
    let pb::ReopenTaskStart {
        id,
        request_id: request_id_value,
    } = start;
    Ok(task::ReopenTask {
        id: parse("id", &id)?,
        request_id: Some(request_id(request_id_value)?),
        request_fingerprint: Some(request_fingerprint),
    })
}

fn task_content_edit(edit: pb::TaskContentEdit) -> Result<task::EditTaskContent, Status> {
    match required("content", edit.content)? {
        pb::task_content_edit::Content::Structured(value) => {
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
        pb::task_content_edit::Content::Append(value) => {
            let title = value
                .title
                .map(TaskTitle::try_new)
                .transpose()
                .map_err(|error| invalid("content.title", error))?;
            task::EditTaskContent::append_shorthand(title, TaskPrompt::new(value.prompt))
                .map_err(|error| invalid("content", error))
        }
        pb::task_content_edit::Content::Replace(value) => Ok(
            task::EditTaskContent::replace_shorthand(TaskPrompt::new(value)),
        ),
    }
}

fn task_lanes(lanes: pb::TaskLanes) -> Result<task::TaskLanes, Status> {
    task::TaskLanes::try_new(
        lanes.goals,
        lanes.context,
        lanes.constraints,
        lanes.done_when,
    )
    .map_err(|error| invalid("lanes", error))
}

fn effort_tier(value: i32) -> Result<EffortTier, Status> {
    match pb::EffortTier::try_from(value).ok() {
        Some(pb::EffortTier::Low) => Ok(EffortTier::Low),
        Some(pb::EffortTier::Medium) => Ok(EffortTier::Medium),
        Some(pb::EffortTier::High) => Ok(EffortTier::High),
        Some(pb::EffortTier::Highest) => Ok(EffortTier::Highest),
        Some(pb::EffortTier::Unspecified) | None => Err(invalid("effort", "must be specified")),
    }
}

fn priority_tier(value: i32) -> Result<PriorityTier, Status> {
    match pb::PriorityTier::try_from(value).ok() {
        Some(pb::PriorityTier::Low) => Ok(PriorityTier::Low),
        Some(pb::PriorityTier::Medium) => Ok(PriorityTier::Medium),
        Some(pb::PriorityTier::High) => Ok(PriorityTier::High),
        Some(pb::PriorityTier::Highest) => Ok(PriorityTier::Highest),
        Some(pb::PriorityTier::Unspecified) | None => Err(invalid("priority", "must be specified")),
    }
}

fn order_spec(order: pb::OrderSpec) -> Result<task::OrderSpec, Status> {
    Ok(task::OrderSpec {
        field: match pb::OrderField::try_from(order.field).ok() {
            Some(pb::OrderField::Created) => task::OrderField::Created,
            Some(pb::OrderField::Id) => task::OrderField::Id,
            Some(pb::OrderField::ProjectId) => task::OrderField::ProjectId,
            Some(pb::OrderField::Unspecified) | None => {
                return Err(invalid("order.field", "must be specified"));
            }
        },
        direction: match pb::OrderDirection::try_from(order.direction).ok() {
            Some(pb::OrderDirection::Asc) => task::OrderDirection::Asc,
            Some(pb::OrderDirection::Desc) => task::OrderDirection::Desc,
            Some(pb::OrderDirection::Unspecified) | None => {
                return Err(invalid("order.direction", "must be specified"));
            }
        },
    })
}

fn status_filter(value: i32) -> Result<task::StatusFilter, Status> {
    match pb::TaskStatusFilter::try_from(value).ok() {
        Some(pb::TaskStatusFilter::Active) => Ok(task::StatusFilter::Exact(TaskStatus::Active)),
        Some(pb::TaskStatusFilter::Done) => Ok(task::StatusFilter::Exact(TaskStatus::Done)),
        Some(pb::TaskStatusFilter::Cancelled) => {
            Ok(task::StatusFilter::Exact(TaskStatus::Cancelled))
        }
        Some(pb::TaskStatusFilter::All) => Ok(task::StatusFilter::All),
        Some(pb::TaskStatusFilter::Unspecified) | None => {
            Err(invalid("status", "must be specified"))
        }
    }
}

fn effort_edit(value: Option<pb::EffortEdit>) -> Result<FieldUpdate<EffortTier>, Status> {
    let Some(value) = value else {
        return Ok(FieldUpdate::Unchanged);
    };
    match required("effort.operation", value.operation)? {
        pb::effort_edit::Operation::Set(value) => Ok(FieldUpdate::Update(effort_tier(value)?)),
        pb::effort_edit::Operation::Clear(_) => Ok(FieldUpdate::Clear),
    }
}

fn priority_edit(value: Option<pb::PriorityEdit>) -> Result<FieldUpdate<PriorityTier>, Status> {
    let Some(value) = value else {
        return Ok(FieldUpdate::Unchanged);
    };
    match required("priority.operation", value.operation)? {
        pb::priority_edit::Operation::Set(value) => Ok(FieldUpdate::Update(priority_tier(value)?)),
        pb::priority_edit::Operation::Clear(_) => Ok(FieldUpdate::Clear),
    }
}

fn task_lane(value: i32) -> Result<task::TaskLane, Status> {
    match pb::TaskLane::try_from(value).ok() {
        Some(pb::TaskLane::Goal) => Ok(task::TaskLane::Goal),
        Some(pb::TaskLane::Context) => Ok(task::TaskLane::Context),
        Some(pb::TaskLane::Constraint) => Ok(task::TaskLane::Constraint),
        Some(pb::TaskLane::DoneWhen) => Ok(task::TaskLane::DoneWhen),
        Some(pb::TaskLane::Unspecified) | None => Err(invalid("lane", "must be specified")),
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

fn request_id(value: String) -> Result<task::TaskRequestId, Status> {
    task::TaskRequestId::try_new(value).map_err(|error| invalid("request_id", error))
}

fn revision(value: String) -> Result<ContentRevision, Status> {
    ContentRevision::try_new(value).map_err(|error| invalid("expected_revision", error))
}

fn ensure_update_bounds(request: &pb::UpdateTaskRequest) -> Result<(), Status> {
    if let Some(content) = request.content.as_ref()
        && let Some(pb::task_content_edit::Content::Structured(edit)) = content.content.as_ref()
    {
        ensure_lanes(
            "content.lanes",
            edit.additions.as_ref(),
            edit.removals.len(),
        )?;
    }
    ensure_count(
        "blocked_by",
        collection_value_count(request.blocked_by.as_ref()),
        TASK_COLLECTION_VALUES_MAX,
    )?;
    ensure_count(
        "tags",
        collection_value_count(request.tags.as_ref()),
        TASK_COLLECTION_VALUES_MAX,
    )
}

fn ensure_lanes(
    field: &str,
    lanes: Option<&pb::TaskLanes>,
    additional: usize,
) -> Result<(), Status> {
    let count = lanes.map_or(0, |lanes| {
        lanes
            .goals
            .len()
            .saturating_add(lanes.context.len())
            .saturating_add(lanes.constraints.len())
            .saturating_add(lanes.done_when.len())
    });
    ensure_count(
        field,
        count.saturating_add(additional),
        TASK_LANE_VALUES_MAX,
    )
}

fn collection_value_count(edit: Option<&pb::StringCollectionEdit>) -> usize {
    match edit.and_then(|edit| edit.operation.as_ref()) {
        Some(
            pb::string_collection_edit::Operation::Append(values)
            | pb::string_collection_edit::Operation::Replace(values),
        ) => values.values.len(),
        Some(pb::string_collection_edit::Operation::Clear(_)) | None => 0,
    }
}

fn ensure_count(field: &str, count: usize, max: usize) -> Result<(), Status> {
    if count > max {
        return Err(invalid(field, format!("may contain at most {max} values")));
    }
    Ok(())
}

fn request_fingerprint<T>(
    request: &T,
    strip_request_id: impl FnOnce(&mut T),
) -> task::TaskRequestFingerprint
where
    T: Message + Clone,
{
    let mut request = request.clone();
    strip_request_id(&mut request);
    task::TaskRequestFingerprint::from_digest(*blake3::hash(&request.encode_to_vec()).as_bytes())
}
