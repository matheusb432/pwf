use std::{error::Error, str::FromStr};

use pwf_models::{
    revision::ContentRevision,
    task::{
        BlockedBy, CommitRanges, EffortTier, PriorityTier, Tag, Task, TaskId, TaskPrompt,
        TaskStatus, TaskTags, TaskTimestamp, TaskTitle,
    },
};

use super::response::{effort_tier_value, priority_tier_value, task_status_value};
use crate::pb;

impl From<Task> for pb::GetTaskResponse {
    fn from(task: Task) -> Self {
        Self {
            task: Some(task.into()),
        }
    }
}

impl From<Task> for pb::Task {
    fn from(task: Task) -> Self {
        Self {
            id: task.id.into_string(),
            title: task.title.into_inner(),
            status: task_status_value(task.status),
            prompt: task.prompt.into_string(),
            created_at: task.created_at.as_ref().map(ToString::to_string),
            completed_at: task.completed_at.as_ref().map(ToString::to_string),
            commits: task.commits.map(CommitRanges::into_inner),
            tags: task
                .tags
                .map(|tags| tags.into_iter().map(Tag::into_string).collect())
                .unwrap_or_default(),
            effort: task.effort.map(effort_tier_value),
            priority: task.priority.map(priority_tier_value),
            blocked_by: task
                .blocked_by
                .map(|ids| ids.into_iter().map(TaskId::into_string).collect())
                .unwrap_or_default(),
            revision: task.revision.into_inner(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeGetTaskResponseError {
    #[error("task response is empty")]
    MissingTask,
    #[error("invalid task {field}: {source}")]
    Field {
        field: &'static str,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
    #[error("invalid task {field} enum value {value}")]
    Enum { field: &'static str, value: i32 },
}

impl TryFrom<pb::GetTaskResponse> for Task {
    type Error = DecodeGetTaskResponseError;

    fn try_from(response: pb::GetTaskResponse) -> Result<Self, Self::Error> {
        response
            .task
            .ok_or(DecodeGetTaskResponseError::MissingTask)?
            .try_into()
    }
}

impl TryFrom<pb::Task> for Task {
    type Error = DecodeGetTaskResponseError;

    fn try_from(value: pb::Task) -> Result<Self, Self::Error> {
        let id = TaskId::try_new(value.id).map_err(|error| invalid("id", error))?;
        let title = TaskTitle::try_new(value.title).map_err(|error| invalid("title", error))?;
        let status = match pb::TaskStatus::try_from(value.status).ok() {
            Some(pb::TaskStatus::Active) => TaskStatus::Active,
            Some(pb::TaskStatus::Done) => TaskStatus::Done,
            Some(pb::TaskStatus::Cancelled) => TaskStatus::Cancelled,
            _ => {
                return Err(DecodeGetTaskResponseError::Enum {
                    field: "status",
                    value: value.status,
                });
            }
        };
        let tags = if value.tags.is_empty() {
            None
        } else {
            let tags = value
                .tags
                .into_iter()
                .map(Tag::try_from)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| invalid("tags", error))?;
            Some(TaskTags::try_new(tags).map_err(|error| invalid("tags", error))?)
        };
        let blocked_by = if value.blocked_by.is_empty() {
            None
        } else {
            let ids = value
                .blocked_by
                .into_iter()
                .map(TaskId::try_new)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| invalid("blocked_by", error))?;
            Some(BlockedBy::try_new(ids).map_err(|error| invalid("blocked_by", error))?)
        };
        let effort = value
            .effort
            .map(|value| match pb::EffortTier::try_from(value).ok() {
                Some(pb::EffortTier::Low) => Ok(EffortTier::Low),
                Some(pb::EffortTier::Medium) => Ok(EffortTier::Medium),
                Some(pb::EffortTier::High) => Ok(EffortTier::High),
                Some(pb::EffortTier::Highest) => Ok(EffortTier::Highest),
                _ => Err(DecodeGetTaskResponseError::Enum {
                    field: "effort",
                    value,
                }),
            })
            .transpose()?;
        let priority = value
            .priority
            .map(|value| match pb::PriorityTier::try_from(value).ok() {
                Some(pb::PriorityTier::Low) => Ok(PriorityTier::Low),
                Some(pb::PriorityTier::Medium) => Ok(PriorityTier::Medium),
                Some(pb::PriorityTier::High) => Ok(PriorityTier::High),
                Some(pb::PriorityTier::Highest) => Ok(PriorityTier::Highest),
                _ => Err(DecodeGetTaskResponseError::Enum {
                    field: "priority",
                    value,
                }),
            })
            .transpose()?;
        Ok(Task {
            id,
            title,
            status,
            prompt: TaskPrompt::new(value.prompt),
            created_at: value
                .created_at
                .as_deref()
                .map(|raw| parse::<TaskTimestamp>("created_at", raw))
                .transpose()?,
            completed_at: value
                .completed_at
                .as_deref()
                .map(|raw| parse::<TaskTimestamp>("completed_at", raw))
                .transpose()?,
            commits: value
                .commits
                .map(|raw| CommitRanges::try_new(raw).map_err(|error| invalid("commits", error)))
                .transpose()?,
            tags,
            effort,
            priority,
            blocked_by,
            revision: ContentRevision::try_new(value.revision)
                .map_err(|error| invalid("revision", error))?,
        })
    }
}

fn parse<T: FromStr>(field: &'static str, raw: &str) -> Result<T, DecodeGetTaskResponseError>
where
    T::Err: Error + Send + Sync + 'static,
{
    raw.parse().map_err(|error| invalid(field, error))
}

fn invalid(
    field: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> DecodeGetTaskResponseError {
    DecodeGetTaskResponseError::Field {
        field,
        source: Box::new(source),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response() -> pb::GetTaskResponse {
        pb::GetTaskResponse {
            task: Some(pb::Task {
                id: "FOO-0001".to_string(),
                title: "parsed task".to_string(),
                status: pb::TaskStatus::Done as i32,
                prompt: "\n  authored body  \n".to_string(),
                created_at: Some("2026-07-26T12:34:56Z".to_string()),
                completed_at: Some("2026-08-12T12:34:56Z".to_string()),
                commits: Some("a..b, c..d".to_string()),
                tags: vec!["rust".to_string(), "sqlite".to_string()],
                effort: Some(pb::EffortTier::High as i32),
                priority: Some(pb::PriorityTier::Highest as i32),
                blocked_by: vec!["AUX-0014".to_string()],
                revision: "a".repeat(64),
            }),
        }
    }

    #[test]
    fn task_conversion_moves_owned_string_allocations() {
        let response = response();
        let wire = response.task.as_ref().unwrap();
        let pointers = [
            wire.id.as_ptr(),
            wire.title.as_ptr(),
            wire.prompt.as_ptr(),
            wire.commits.as_ref().unwrap().as_ptr(),
            wire.tags[0].as_ptr(),
            wire.blocked_by[0].as_ptr(),
            wire.revision.as_ptr(),
        ];
        let task = Task::try_from(response).unwrap();
        let encoded = pb::GetTaskResponse::from(task);
        let wire = encoded.task.unwrap();
        assert_eq!(
            pointers,
            [
                wire.id.as_ptr(),
                wire.title.as_ptr(),
                wire.prompt.as_ptr(),
                wire.commits.as_ref().unwrap().as_ptr(),
                wire.tags[0].as_ptr(),
                wire.blocked_by[0].as_ptr(),
                wire.revision.as_ptr()
            ]
        );
    }

    #[test]
    fn get_task_round_trip_retains_semantic_fields_and_full_timestamps() {
        let expected = response();
        let task = Task::try_from(expected.clone()).unwrap();
        assert_eq!(pb::GetTaskResponse::from(task), expected);
    }

    #[test]
    fn get_task_decoding_rejects_invalid_boundary_values() {
        assert!(matches!(
            Task::try_from(pb::GetTaskResponse { task: None }),
            Err(DecodeGetTaskResponseError::MissingTask)
        ));
        for field in [
            "id",
            "title",
            "status",
            "created_at",
            "completed_at",
            "commits",
            "tags",
            "effort",
            "priority",
            "blocked_by",
            "revision",
        ] {
            let mut response = response();
            let task = response.task.as_mut().unwrap();
            match field {
                "id" => task.id = "foo1".to_string(),
                "title" => task.title = "x".repeat(201),
                "status" => task.status = 0,
                "created_at" => task.created_at = Some("yesterday".to_string()),
                "completed_at" => task.completed_at = Some("tomorrow".to_string()),
                "commits" => task.commits = Some(String::new()),
                "tags" => task.tags = vec!["Bad Tag".to_string()],
                "effort" => task.effort = Some(0),
                "priority" => task.priority = Some(999),
                "blocked_by" => task.blocked_by = vec!["aux14".to_string()],
                "revision" => task.revision = String::new(),
                _ => {}
            }
            let error = Task::try_from(response).unwrap_err().to_string();
            assert!(error.contains(field), "{field}: {error}");
        }
    }

    #[test]
    fn get_task_decoding_preserves_absence_and_placeholder_prompts() {
        let response = pb::GetTaskResponse {
            task: Some(pb::Task {
                id: "FOO-0001".to_string(),
                title: "placeholder".to_string(),
                status: pb::TaskStatus::Active as i32,
                prompt: "TODO".to_string(),
                revision: "a".repeat(64),
                ..Default::default()
            }),
        };
        let task = Task::try_from(response.clone()).unwrap();
        assert_eq!(pb::GetTaskResponse::from(task), response);
    }
}
