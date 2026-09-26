//! Parses plugin request notifications and encodes their replies.
//!
//! Lua sends `rpcnotify(channel, "request", id, operation, params)`. The child answers with an
//! `nvim_exec_lua` notification that calls `require("pwf.client").resolve(id, reply)`, where
//! `reply` is `{ status = "ok", value = ... }` or `{ status = "failed", message = ... }`.

use rmpv::Value;
use serde::de::DeserializeOwned;

use crate::{OperationError, task_file::TaskFileParams, task_list::ListTasksParams};

pub(crate) const REQUEST_NOTIFICATION: &str = "request";
pub(crate) const REPLY_METHOD: &str = "nvim_exec_lua";
const RESOLVE_LUA: &str = "return require('pwf.client').resolve(...)";

#[derive(Debug)]
pub(crate) enum Operation {
    ListTasks(ListTasksParams),
    TaskFile(TaskFileParams),
}

#[derive(Debug)]
pub(crate) struct Request {
    pub(crate) id: u64,
    pub(crate) operation: Result<Operation, OperationError>,
}

/// Returns `None` when the notification carries no request ID to reply to.
pub(crate) fn parse_request(args: Vec<Value>) -> Option<Request> {
    let mut args = args.into_iter();
    let id = args.next()?.as_u64()?;
    let operation = parse_operation(args.next().as_ref(), args.next());
    Some(Request { id, operation })
}

fn parse_operation(
    name: Option<&Value>,
    params: Option<Value>,
) -> Result<Operation, OperationError> {
    let name = name
        .and_then(Value::as_str)
        .ok_or_else(|| OperationError::InvalidRequest("missing operation name".to_string()))?;
    let params = params.unwrap_or_else(|| Value::Map(Vec::new()));
    match name {
        "list_tasks" => decode(name, params).map(Operation::ListTasks),
        "task_file" => decode(name, params).map(Operation::TaskFile),
        unknown => Err(OperationError::InvalidRequest(format!(
            "unknown operation {unknown:?}"
        ))),
    }
}

fn decode<T: DeserializeOwned>(operation: &str, params: Value) -> Result<T, OperationError> {
    rmpv::ext::from_value(params)
        .map_err(|error| OperationError::InvalidRequest(format!("{operation}: {error}")))
}

/// Builds the [`REPLY_METHOD`] parameters that deliver one finished request to Lua.
pub(crate) fn reply_params(id: u64, outcome: Result<Value, OperationError>) -> Vec<Value> {
    let reply = match outcome {
        Ok(value) => vec![
            (Value::from("status"), Value::from("ok")),
            (Value::from("value"), value),
        ],
        Err(error) => vec![
            (Value::from("status"), Value::from("failed")),
            (Value::from("message"), Value::from(error.to_string())),
        ],
    };
    let arguments = vec![Value::from(id), Value::Map(reply)];
    vec![Value::from(RESOLVE_LUA), Value::Array(arguments)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_parameters_still_carry_the_request_id() {
        let request = parse_request(vec![
            Value::from(7),
            Value::from("list_tasks"),
            Value::Map(vec![(Value::from("status"), Value::from("sometimes"))]),
        ])
        .unwrap();

        assert_eq!(request.id, 7);
        assert!(matches!(
            request.operation,
            Err(OperationError::InvalidRequest(message)) if message.starts_with("list_tasks: ")
        ));
    }
}
