//! Lists tasks as picker entries, bounded by a caller-selected cap.

use std::fmt::Write as _;

use pwf_client::{PwfClient, pb};
use pwf_models::task::{TaskListLimit, TaskTags};
use serde::Deserialize;

use crate::OperationError;

const TASK_LIST_PAGE_SIZE: u32 = 256;

/// Decoded from a string: `rmpv` reads unit enum variants only from integers or maps.
#[derive(Debug, Deserialize)]
#[serde(try_from = "String")]
pub(crate) enum StatusScope {
    Active,
    Backlog,
    Done,
    Cancelled,
    All,
}

impl TryFrom<String> for StatusScope {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "active" => Ok(Self::Active),
            "backlog" => Ok(Self::Backlog),
            "done" => Ok(Self::Done),
            "cancelled" => Ok(Self::Cancelled),
            "all" => Ok(Self::All),
            _ => Err(format!(
                "unknown status scope {value:?}; expected active, backlog, done, cancelled, or all"
            )),
        }
    }
}

#[derive(Debug)]
pub(crate) struct TaskListing {
    pub(crate) tasks: Vec<pb::ListedTask>,
    /// Matching tasks beyond the requested limit.
    pub(crate) hidden: u64,
}

pub(crate) async fn execute(
    client: &PwfClient,
    project: Option<String>,
    status: StatusScope,
    limit: TaskListLimit,
) -> Result<TaskListing, OperationError> {
    let status = match status {
        StatusScope::Active => pb::TaskStatusFilter::Active,
        StatusScope::Backlog => pb::TaskStatusFilter::Backlog,
        StatusScope::Done => pb::TaskStatusFilter::Done,
        StatusScope::Cancelled => pb::TaskStatusFilter::Cancelled,
        StatusScope::All => pb::TaskStatusFilter::All,
    };
    let limit = limit.get();
    let mut request = pb::ListTasksRequest {
        project_id: project.clone(),
        number: Some(limit as u64),
        status: Some(status as i32),
        detail: pb::ListDetail::Summary as i32,
        page_size: TASK_LIST_PAGE_SIZE,
        ..Default::default()
    };
    let tasks = client.task();
    let mut page = tasks.list_tasks(request.clone()).await?;
    let hidden = page.hidden;
    let mut listed_tasks = Vec::with_capacity(page.tasks.len());
    // The server returns at most `limit` tasks, so one extra page covers a trailing empty page.
    let mut pages_remaining = limit.div_ceil(TASK_LIST_PAGE_SIZE as usize);
    loop {
        listed_tasks.append(&mut page.tasks);
        let Some(token) = page.next_page_token.take() else {
            break;
        };
        if pages_remaining == 0 {
            return Err(OperationError::UnboundedPages);
        }
        pages_remaining -= 1;
        request.page_token = Some(token);
        page = tasks.list_tasks(request.clone()).await?;
    }
    Ok(TaskListing {
        tasks: listed_tasks,
        hidden,
    })
}

pub(crate) fn task_entry(task: &pb::ListedTask) -> Result<String, OperationError> {
    let mut entry = format!("{} {}", task.id, task.heading);
    if let Some(raw) = task.raw_tags.as_deref() {
        let tags = TaskTags::parse_frontmatter(raw)
            .map_err(|error| OperationError::InvalidTask(format!("{}: {error}", task.id)))?;
        for tag in tags.iter() {
            let _ = write!(entry, " #{tag}");
        }
    }
    let status = match pb::TaskStatus::try_from(task.status) {
        Ok(pb::TaskStatus::Active) => return Ok(entry),
        Ok(pb::TaskStatus::Done) => "done",
        Ok(pb::TaskStatus::Cancelled) => "cancelled",
        Ok(pb::TaskStatus::Backlog) => "backlog",
        Ok(pb::TaskStatus::Unspecified) | Err(_) => {
            return Err(OperationError::InvalidTask(format!(
                "{}: unknown status {}",
                task.id, task.status
            )));
        }
    };
    let _ = write!(entry, " [{status}]");
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed_task(status: pb::TaskStatus, raw_tags: Option<&str>) -> pb::ListedTask {
        pb::ListedTask {
            id: "PWF-0007".to_string(),
            heading: "ship the picker".to_string(),
            status: status as i32,
            raw_tags: raw_tags.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn entry_lists_tags_and_marks_closed_statuses() {
        assert_eq!(
            task_entry(&listed_task(pb::TaskStatus::Active, Some("[nvim, ux]"))).unwrap(),
            "PWF-0007 ship the picker #nvim #ux"
        );
        assert_eq!(
            task_entry(&listed_task(pb::TaskStatus::Done, None)).unwrap(),
            "PWF-0007 ship the picker [done]"
        );
    }
}
