//! Serves the pwf Neovim plugin as a MessagePack-RPC child process on stdio.
//!
//! Neovim owns the process: the Lua plugin starts it on first use, and it exits when Neovim closes
//! the channel. Stdout carries only protocol messages; diagnostics go to stderr.

mod msgpack_rpc;
mod note_list;
mod project_scope;
mod protocol;
mod record_list;
mod task_list;

use std::{sync::Arc, time::Duration};

use anyhow::bail;
use clap::Parser;
use pwf_client::{ClientError, ConnectError, PwfClient};
use rmpv::Value;
use tokio::sync::{Mutex, Notify, Semaphore};

use crate::{
    msgpack_rpc::Notification,
    protocol::{Operation, REPLY_METHOD, REQUEST_NOTIFICATION},
};

/// Plugin protocol version this executable serves; `lua/pwf/client.lua` requests it on start.
const PROTOCOL_VERSION: u32 = 5;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const REQUESTS_IN_FLIGHT_MAX: usize = 16;

/// Serve the pwf Neovim plugin over MessagePack-RPC on stdio.
#[derive(Parser)]
#[command(name = "pwf-nvim", version)]
struct Arguments {
    /// Plugin protocol version requested by the Lua plugin.
    #[arg(long)]
    protocol: u32,
}

/// Parses process arguments and serves plugin requests until Neovim closes the channel.
///
/// Returns early after a release mismatch with `pwf-server`, so the plugin starts the installed
/// executable on its next request.
pub async fn run() -> anyhow::Result<()> {
    let arguments = Arguments::parse();
    if arguments.protocol != PROTOCOL_VERSION {
        bail!(
            "pwf-nvim serves plugin protocol {PROTOCOL_VERSION}, but the plugin requested {}. \
             Install matching pwf-nvim and plugin versions.",
            arguments.protocol
        );
    }
    let state = Arc::new(State {
        client: Mutex::new(None),
        requests_in_flight: Arc::new(Semaphore::new(REQUESTS_IN_FLIGHT_MAX)),
        shutdown: Notify::new(),
    });
    let writer = Arc::new(msgpack_rpc::Writer::new(tokio::io::stdout()));
    let served = msgpack_rpc::serve(tokio::io::stdin(), &writer, |notification| {
        dispatch(&state, &writer, notification);
    });
    tokio::select! {
        result = served => Ok(result?),
        () = state.shutdown.notified() => Ok(()),
    }
}

#[derive(Debug, thiserror::Error)]
enum OperationError {
    #[error(
        "Cannot connect to pwf-server.\nRun `pwf doctor` to identify the cause and recovery action."
    )]
    Connect(#[source] ConnectError),
    #[error("{}", client_error_message(.0))]
    Client(#[from] ClientError),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("pwf-server returned an invalid task: {0}")]
    InvalidTask(String),
    #[error(transparent)]
    RecordList(#[from] record_list::RecordListError),
    #[error("could not finish preparing picker records: {0}")]
    RecordWorker(#[from] tokio::task::JoinError),
    #[error("pwf-server returned more task-list pages than the requested limit allows")]
    UnboundedPages,
    #[error("pwf-server did not finish within {0} seconds")]
    TimedOut(u64),
    #[error(
        "pwf-nvim is already serving {REQUESTS_IN_FLIGHT_MAX} requests; retry after they finish"
    )]
    Busy,
}

fn client_error_message(error: &ClientError) -> String {
    match error {
        ClientError::Rpc(status) => status.message().to_string(),
        other => other.to_string(),
    }
}

struct State {
    /// Connected on first use; a failed connection is retried by the next request.
    client: Mutex<Option<Arc<PwfClient>>>,
    requests_in_flight: Arc<Semaphore>,
    shutdown: Notify,
}

impl State {
    async fn client(&self) -> Result<Arc<PwfClient>, OperationError> {
        let mut client = self.client.lock().await;
        if let Some(client) = client.as_ref() {
            return Ok(client.clone());
        }
        let connected = Arc::new(
            PwfClient::connect_local()
                .await
                .map_err(OperationError::Connect)?,
        );
        *client = Some(connected.clone());
        Ok(connected)
    }

    async fn execute(&self, operation: Operation) -> Result<Value, OperationError> {
        let client = self.client().await?;
        match operation {
            Operation::ListRecords(params) => {
                Ok(record_list::execute(&client, params).await?.into())
            }
        }
    }
}

type StdoutWriter = msgpack_rpc::Writer<tokio::io::Stdout>;

/// Runs each request on its own task so the reader keeps accepting notifications.
fn dispatch(state: &Arc<State>, writer: &Arc<StdoutWriter>, notification: Notification) {
    match notification.method.as_str() {
        REQUEST_NOTIFICATION => {}
        "nvim_error_event" => {
            let message = notification.params.get(1).and_then(Value::as_str);
            eprintln!(
                "pwf-nvim: Neovim rejected a reply: {}",
                message.unwrap_or("no message")
            );
            return;
        }
        unknown => {
            eprintln!("pwf-nvim: ignoring unknown notification {unknown:?}");
            return;
        }
    }
    let Some(request) = protocol::parse_request(notification.params) else {
        eprintln!("pwf-nvim: ignoring a request without a numeric ID");
        return;
    };
    let permit = state.requests_in_flight.clone().try_acquire_owned();
    let state = state.clone();
    let writer = writer.clone();
    tokio::spawn(async move {
        let outcome = match (permit, request.operation) {
            (Err(_), _) => Err(OperationError::Busy),
            (Ok(_permit), Err(error)) => Err(error),
            (Ok(_permit), Ok(operation)) => {
                tokio::time::timeout(REQUEST_TIMEOUT, state.execute(operation))
                    .await
                    .unwrap_or(Err(OperationError::TimedOut(REQUEST_TIMEOUT.as_secs())))
            }
        };
        let release_mismatch = matches!(
            &outcome,
            Err(OperationError::Client(error)) if error.is_release_mismatch()
        );
        let reply = protocol::reply_params(request.id, outcome);
        if let Err(error) = writer.notify(REPLY_METHOD, reply).await {
            eprintln!("pwf-nvim: could not deliver reply {}: {error}", request.id);
        }
        if release_mismatch {
            state.shutdown.notify_one();
        }
    });
}
