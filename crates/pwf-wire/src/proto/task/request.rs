//! Protobuf request mappings for task operations.

use pwf_models::{
    project::ProjectId,
    revision::ContentRevision,
    task::{
        BlockedBy, CommitRanges, EffortTier, PriorityTier, Tag, TaskBody, TaskId, TaskReport,
        TaskStatus, TaskTags, TaskTitle,
        order::{OrderDirection, OrderField, OrderSpec},
    },
};
use tonic::Status;

use super::super::{collection_edit, invalid, parse, required};
use crate::{patch_field::PatchField, pb, task};

const TASK_COLLECTION_VALUES_MAX: usize = 64;

impl TryFrom<pb::BacklogTaskRequest> for TaskId {
    type Error = Status;

    fn try_from(request: pb::BacklogTaskRequest) -> Result<Self, Self::Error> {
        parse("id", &request.id)
    }
}

pub fn create_task_request(request: pb::CreateTaskRequest) -> Result<task::AddTask, Status> {
    ensure_count(
        "blocked_by",
        request.blocked_by.len(),
        TASK_COLLECTION_VALUES_MAX,
    )?;
    ensure_count("tags", request.tags.len(), TASK_COLLECTION_VALUES_MAX)?;

    let pb::CreateTaskRequest {
        project_id,
        content,
        blocked_by,
        effort,
        tags,
        priority,
    } = request;

    let body = match required("content", content)? {
        pb::create_task_request::Content::Shorthand(shorthand) => {
            task::AddTaskBody::from_shorthand(shorthand)
        }
        pb::create_task_request::Content::Body(body) => task::AddTaskBody::from_body(
            TaskTitle::try_new(body.title).map_err(|error| invalid("content.title", error))?,
            TaskBody::new(body.body),
        ),
    };
    Ok(task::AddTask {
        project_id: ProjectId::try_new(project_id).map_err(|error| invalid("project_id", error))?,
        body,
        blocked_by: blocked_by_values(blocked_by)?,
        effort: effort.map(effort_tier).transpose()?,
        tags: task_tag_values(tags)?,
        priority: priority.map(priority_tier).transpose()?,
    })
}

impl TryFrom<pb::CreateTaskFromFileRequest> for task::AddTaskFromFile {
    type Error = Status;

    fn try_from(request: pb::CreateTaskFromFileRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            project_id: ProjectId::try_new(request.project_id)
                .map_err(|error| invalid("project_id", error))?,
            source_file: request.source_file.into(),
            title: request
                .title
                .map(|title| TaskTitle::try_new(title).map_err(|error| invalid("title", error)))
                .transpose()?,
        })
    }
}

impl TryFrom<pb::CloneTaskRequest> for task::CloneTask {
    type Error = Status;

    fn try_from(request: pb::CloneTaskRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            id: TaskId::try_new(request.id).map_err(|error| invalid("id", error))?,
            project_id: task::ClonedTaskProjectId::new(
                request
                    .project_id
                    .map(|id| ProjectId::try_new(id).map_err(|error| invalid("project_id", error)))
                    .transpose()?,
            ),
        })
    }
}

pub fn cancel_task_request(request: pb::CancelTaskRequest) -> Result<task::CancelTask, Status> {
    ensure_count("commits", request.commits.len(), TASK_COLLECTION_VALUES_MAX)?;
    let pb::CancelTaskRequest {
        id,
        report,
        commits,
        expected_revision,
    } = request;
    Ok(task::CancelTask {
        id: parse::<TaskId>("id", &id)?,
        report: parse::<TaskReport>("report", &report)?,
        commits: CommitRanges::from_inputs(&commits),
        expected_revision: expected_revision.map(revision).transpose()?,
    })
}

pub fn complete_task_request(
    request: pb::CompleteTaskRequest,
) -> Result<task::CompleteTask, Status> {
    ensure_count("commits", request.commits.len(), TASK_COLLECTION_VALUES_MAX)?;
    let pb::CompleteTaskRequest {
        id,
        report,
        commits,
        expected_revision,
    } = request;
    Ok(task::CompleteTask {
        id: parse::<TaskId>("id", &id)?,
        report: report
            .as_deref()
            .map(|value| parse::<TaskReport>("report", value))
            .transpose()?,
        commits: CommitRanges::from_inputs(&commits),
        expected_revision: expected_revision.map(revision).transpose()?,
    })
}

pub fn update_task_request(request: pb::UpdateTaskRequest) -> Result<task::EditTask, Status> {
    ensure_update_bounds(&request)?;
    let pb::UpdateTaskRequest {
        id,
        content,
        blocked_by,
        effort,
        tags,
        priority,
        expected_revision,
    } = request;
    let content = content.map(task_content_edit).transpose()?.into();
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
    })
}

impl TryFrom<pb::GetTaskRequest> for TaskId {
    type Error = Status;

    fn try_from(request: pb::GetTaskRequest) -> Result<Self, Self::Error> {
        request.id.try_into().map_err(|error| invalid("id", error))
    }
}

impl TryFrom<pb::GetTaskRecordRequest> for TaskId {
    type Error = Status;

    fn try_from(request: pb::GetTaskRecordRequest) -> Result<Self, Self::Error> {
        request.id.try_into().map_err(|error| invalid("id", error))
    }
}

impl TryFrom<pb::GetTaskBodySectionsRequest> for task::GetTaskBodySections {
    type Error = Status;

    fn try_from(request: pb::GetTaskBodySectionsRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            project_id: request
                .project_id
                .map(|project_id| {
                    ProjectId::try_new(project_id).map_err(|error| invalid("project_id", error))
                })
                .transpose()?,
        })
    }
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

    let detail = match pb::ListDetail::try_from(request.detail).ok() {
        Some(pb::ListDetail::Summary) => task::ListDetail::Summary,
        Some(pb::ListDetail::Detailed) => task::ListDetail::Detailed,
        Some(pb::ListDetail::Preview) => task::ListDetail::Preview,
        Some(pb::ListDetail::Unspecified) | None => {
            return Err(invalid("detail", "must be specified"));
        }
    };
    Ok(task::ListTasks {
        project_id: request
            .project_id
            .as_deref()
            .map(|value| parse::<ProjectId>("project_id", value))
            .transpose()?,
        all: request.all,
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
    let pb::DeleteTaskStart { id } = start;
    Ok(task::DeleteTask {
        id: parse("id", &id)?,
    })
}

pub fn activate_task_start(start: pb::ActivateTaskStart) -> Result<task::ActivateTask, Status> {
    let pb::ActivateTaskStart { id } = start;
    Ok(task::ActivateTask {
        id: parse("id", &id)?,
    })
}

fn task_content_edit(edit: pb::TaskContentEdit) -> Result<task::EditTaskContent, Status> {
    match required("content", edit.content)? {
        pb::task_content_edit::Content::Title(value) => TaskTitle::try_new(value)
            .map(task::EditTaskContent::title)
            .map_err(|error| invalid("content.title", error)),
        pb::task_content_edit::Content::Append(value) => {
            let title = value
                .title
                .map(TaskTitle::try_new)
                .transpose()
                .map_err(|error| invalid("content.title", error))?
                .into();
            task::EditTaskContent::append_shorthand(title, value.body)
                .map_err(|error| invalid("content", error))
        }
        pb::task_content_edit::Content::Replace(value) => {
            Ok(task::EditTaskContent::replace_shorthand(value))
        }
        pb::task_content_edit::Content::Body(value) => {
            let title = value
                .title
                .map(TaskTitle::try_new)
                .transpose()
                .map_err(|error| invalid("content.title", error))?
                .into();
            Ok(task::EditTaskContent::replace_body(
                title,
                TaskBody::new(value.body),
            ))
        }
    }
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

fn order_spec(order: pb::OrderSpec) -> Result<OrderSpec, Status> {
    Ok(OrderSpec {
        field: match pb::OrderField::try_from(order.field).ok() {
            Some(pb::OrderField::Created) => OrderField::Created,
            Some(pb::OrderField::Id) => OrderField::Id,
            Some(pb::OrderField::ProjectId) => OrderField::ProjectId,
            Some(pb::OrderField::Priority) => OrderField::Priority,
            Some(pb::OrderField::Effort) => OrderField::Effort,
            Some(pb::OrderField::Title) => OrderField::Title,
            Some(pb::OrderField::Unspecified) | None => {
                return Err(invalid("order.field", "must be specified"));
            }
        },
        direction: match pb::OrderDirection::try_from(order.direction).ok() {
            Some(pb::OrderDirection::Asc) => OrderDirection::Asc,
            Some(pb::OrderDirection::Desc) => OrderDirection::Desc,
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
        Some(pb::TaskStatusFilter::Backlog) => Ok(task::StatusFilter::Exact(TaskStatus::Backlog)),
        Some(pb::TaskStatusFilter::Cancelled) => {
            Ok(task::StatusFilter::Exact(TaskStatus::Cancelled))
        }
        Some(pb::TaskStatusFilter::All) => Ok(task::StatusFilter::All),
        Some(pb::TaskStatusFilter::Unspecified) | None => {
            Err(invalid("status", "must be specified"))
        }
    }
}

fn effort_edit(value: Option<pb::EffortEdit>) -> Result<PatchField<EffortTier>, Status> {
    let Some(value) = value else {
        return Ok(PatchField::NoAction);
    };
    match required("effort.operation", value.operation)? {
        pb::effort_edit::Operation::Set(value) => Ok(PatchField::Set(effort_tier(value)?)),
        pb::effort_edit::Operation::Clear(_) => Ok(PatchField::Clear),
    }
}

fn priority_edit(value: Option<pb::PriorityEdit>) -> Result<PatchField<PriorityTier>, Status> {
    let Some(value) = value else {
        return Ok(PatchField::NoAction);
    };
    match required("priority.operation", value.operation)? {
        pb::priority_edit::Operation::Set(value) => Ok(PatchField::Set(priority_tier(value)?)),
        pb::priority_edit::Operation::Clear(_) => Ok(PatchField::Clear),
    }
}

fn blocked_by_values(values: Vec<String>) -> Result<Option<BlockedBy>, Status> {
    if values.is_empty() {
        return Ok(None);
    }
    let values = values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            TaskId::try_from(value).map_err(|error| invalid(&format!("blocked_by[{index}]"), error))
        })
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
            Tag::try_from(value).map_err(|error| invalid(&format!("tags[{index}]"), error))
        })
        .collect::<Result<Vec<_>, _>>()?;
    TaskTags::try_new(values)
        .map(Some)
        .map_err(|error| invalid("tags", error))
}

fn revision(value: String) -> Result<ContentRevision, Status> {
    ContentRevision::try_new(value).map_err(|error| invalid("expected_revision", error))
}

fn ensure_update_bounds(request: &pb::UpdateTaskRequest) -> Result<(), Status> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_read_requests_consume_ids_and_preserve_shorthand() {
        let id = "FOO-0001".to_string();
        let pointer = id.as_ptr();
        let parsed = TaskId::try_from(pb::GetTaskRequest { id }).unwrap();
        assert_eq!(parsed.as_ref().as_ptr(), pointer);
        let id = parsed.into_string();
        let parsed = TaskId::try_from(pb::GetTaskRecordRequest { id }).unwrap();
        assert_eq!(parsed.as_ref().as_ptr(), pointer);
        for raw in ["foo1", " FOO-1 ", "foo-0001"] {
            assert_eq!(
                TaskId::try_from(pb::GetTaskRequest {
                    id: raw.to_string()
                })
                .unwrap(),
                parsed
            );
            assert_eq!(
                TaskId::try_from(pb::GetTaskRecordRequest {
                    id: raw.to_string()
                })
                .unwrap(),
                parsed
            );
        }
        for raw in ["", "   ", "invalid", "FOO-10000"] {
            assert_eq!(
                TaskId::try_from(pb::GetTaskRequest {
                    id: raw.to_string()
                })
                .unwrap_err()
                .code(),
                tonic::Code::InvalidArgument
            );
            assert_eq!(
                TaskId::try_from(pb::GetTaskRecordRequest {
                    id: raw.to_string()
                })
                .unwrap_err()
                .code(),
                tonic::Code::InvalidArgument
            );
        }
    }
}
