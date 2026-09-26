//! Resolves the Markdown file that stores a task.

use pwf_client::{pb, task::TaskClient};
use pwf_models::task::TaskId;
use rmpv::Value;
use serde::{Deserialize, Deserializer};

use crate::OperationError;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskFileParams {
    #[serde(deserialize_with = "deserialize_task_id")]
    id: TaskId,
}

#[derive(Debug)]
pub(crate) struct TaskFile {
    path: String,
}

impl From<TaskFile> for Value {
    fn from(file: TaskFile) -> Self {
        Value::Map(vec![(Value::from("path"), Value::from(file.path))])
    }
}

pub(crate) async fn execute(
    tasks: &TaskClient,
    params: TaskFileParams,
) -> Result<TaskFile, OperationError> {
    let response = tasks
        .get_task_record(pb::GetTaskRecordRequest {
            id: params.id.to_string(),
        })
        .await?;
    let record = response.record.ok_or_else(|| {
        OperationError::InvalidTask(format!("{}: pwf-server omitted the task record", params.id))
    })?;
    Ok(TaskFile {
        path: record.locator,
    })
}

fn deserialize_task_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<TaskId, D::Error> {
    let id = String::deserialize(deserializer)?;
    TaskId::try_new(id).map_err(serde::de::Error::custom)
}
