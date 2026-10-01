//! Minimal MessagePack-RPC peer for the Neovim parent process.
//!
//! `pwf-nvim` only exchanges notifications: Neovim notifies it of plugin requests, and it answers
//! with notifications that call Neovim API functions. Neovim reports a failed notified call with
//! an `nvim_error_event` notification. Requests from Neovim receive an error response so a
//! blocking `rpcrequest` cannot hang the editor.

use std::io;

use rmpv::Value;
use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _},
    sync::Mutex,
};

const REQUEST: u64 = 0;
const RESPONSE: u64 = 1;
const NOTIFICATION: u64 = 2;
/// Bounds the read buffer; plugin requests are small parameter maps.
const MESSAGE_BYTES_MAX: usize = 16 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub(crate) struct Notification {
    pub(crate) method: String,
    pub(crate) params: Vec<Value>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ServeError {
    #[error("could not read from Neovim")]
    Read(#[source] io::Error),
    #[error("could not answer a Neovim request")]
    Write(#[source] io::Error),
    #[error("Neovim sent malformed MessagePack")]
    Decode(#[source] rmpv::decode::Error),
    #[error("Neovim sent a message larger than {MESSAGE_BYTES_MAX} bytes")]
    MessageTooLarge,
    #[error("Neovim closed the channel in the middle of a message")]
    Truncated,
}

/// Serializes whole messages onto the shared output stream.
pub(crate) struct Writer<W> {
    output: Mutex<W>,
}

impl<W: AsyncWrite + Unpin> Writer<W> {
    pub(crate) fn new(output: W) -> Self {
        Self {
            output: Mutex::new(output),
        }
    }

    pub(crate) async fn notify(&self, method: &str, params: Vec<Value>) -> io::Result<()> {
        self.write(&Value::Array(vec![
            Value::from(NOTIFICATION),
            Value::from(method),
            Value::Array(params),
        ]))
        .await
    }

    async fn respond_with_error(&self, id: u64, message: String) -> io::Result<()> {
        self.write(&Value::Array(vec![
            Value::from(RESPONSE),
            Value::from(id),
            Value::from(message),
            Value::Nil,
        ]))
        .await
    }

    async fn write(&self, message: &Value) -> io::Result<()> {
        let mut bytes = Vec::new();
        rmpv::encode::write_value(&mut bytes, message).map_err(io::Error::other)?;
        let mut output = self.output.lock().await;
        output.write_all(&bytes).await?;
        output.flush().await
    }
}

/// Reads messages until Neovim closes the input, passing each notification to `on_notification`.
pub(crate) async fn serve<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    mut input: R,
    writer: &Writer<W>,
    mut on_notification: impl FnMut(Notification),
) -> Result<(), ServeError> {
    let mut buffer = Vec::new();
    while let Some(message) = read_message(&mut input, &mut buffer).await? {
        match parse_message(message) {
            Ok(Message::Notification(notification)) => on_notification(notification),
            Ok(Message::Request { id, method }) => writer
                .respond_with_error(
                    id,
                    format!("pwf-nvim accepts only notifications; send {method:?} with rpcnotify"),
                )
                .await
                .map_err(ServeError::Write)?,
            // pwf-nvim sends no requests, so no response can match one.
            Ok(Message::Response) => {}
            Err(reason) => {
                eprintln!("pwf-nvim: ignoring an invalid MessagePack-RPC message: {reason}");
            }
        }
    }
    Ok(())
}

enum Message {
    Request { id: u64, method: String },
    Response,
    Notification(Notification),
}

fn parse_message(message: Value) -> Result<Message, String> {
    let Value::Array(fields) = message else {
        return Err(format!("expected an array, got {message}"));
    };
    let mut fields = fields.into_iter();
    match fields.next().as_ref().and_then(Value::as_u64) {
        Some(REQUEST) => {
            let id = fields.next().as_ref().and_then(Value::as_u64);
            let method = fields
                .next()
                .as_ref()
                .and_then(|method| method.as_str().map(str::to_owned));
            match (id, method) {
                (Some(id), Some(method)) => Ok(Message::Request { id, method }),
                _ => Err("request without an ID and method".to_string()),
            }
        }
        Some(RESPONSE) => Ok(Message::Response),
        Some(NOTIFICATION) => {
            let method = fields
                .next()
                .as_ref()
                .and_then(|method| method.as_str().map(str::to_owned));
            match (method, fields.next()) {
                (Some(method), Some(Value::Array(params))) => {
                    Ok(Message::Notification(Notification { method, params }))
                }
                _ => Err("notification without a method and parameter array".to_string()),
            }
        }
        _ => Err("unknown message type".to_string()),
    }
}

/// Returns the next complete message, or `None` when the input ends between messages.
async fn read_message<R: AsyncRead + Unpin>(
    input: &mut R,
    buffer: &mut Vec<u8>,
) -> Result<Option<Value>, ServeError> {
    loop {
        if !buffer.is_empty() {
            let mut cursor = io::Cursor::new(buffer.as_slice());
            match rmpv::decode::read_value(&mut cursor) {
                Ok(message) => {
                    let consumed = usize::try_from(cursor.position()).unwrap_or(buffer.len());
                    buffer.drain(..consumed);
                    return Ok(Some(message));
                }
                Err(error) if is_incomplete(&error) => {}
                Err(error) => return Err(ServeError::Decode(error)),
            }
        }
        if buffer.len() >= MESSAGE_BYTES_MAX {
            return Err(ServeError::MessageTooLarge);
        }
        if input.read_buf(buffer).await.map_err(ServeError::Read)? == 0 {
            return if buffer.is_empty() {
                Ok(None)
            } else {
                Err(ServeError::Truncated)
            };
        }
    }
}

fn is_incomplete(error: &rmpv::decode::Error) -> bool {
    match error {
        rmpv::decode::Error::InvalidMarkerRead(error)
        | rmpv::decode::Error::InvalidDataRead(error) => {
            error.kind() == io::ErrorKind::UnexpectedEof
        }
        rmpv::decode::Error::DepthLimitExceeded => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(message: &Value) -> Vec<u8> {
        let mut bytes = Vec::new();
        rmpv::encode::write_value(&mut bytes, message).unwrap();
        bytes
    }

    fn notification_bytes(method: &str, param: i64) -> Vec<u8> {
        encode(&Value::Array(vec![
            Value::from(NOTIFICATION),
            Value::from(method),
            Value::Array(vec![Value::from(param)]),
        ]))
    }

    async fn serve_input(
        input: impl AsyncRead + Unpin,
    ) -> (Result<(), ServeError>, Vec<Notification>, Vec<u8>) {
        let writer = Writer::new(Vec::new());
        let mut notifications = Vec::new();
        let result = serve(input, &writer, |notification| {
            notifications.push(notification);
        })
        .await;
        (result, notifications, writer.output.into_inner())
    }

    #[tokio::test]
    async fn notifications_split_across_reads_arrive_whole_and_in_order() {
        let first = notification_bytes("first", 1);
        let second = notification_bytes("second", 2);
        let (head, tail) = first.split_at(first.len() / 2);
        let rest = [tail, second.as_slice()].concat();

        let (result, notifications, _) = serve_input(head.chain(rest.as_slice())).await;

        result.unwrap();
        assert_eq!(
            notifications,
            [
                Notification {
                    method: "first".to_string(),
                    params: vec![Value::from(1)]
                },
                Notification {
                    method: "second".to_string(),
                    params: vec![Value::from(2)]
                },
            ]
        );
    }

    #[tokio::test]
    async fn requests_receive_an_error_response() {
        let request = encode(&Value::Array(vec![
            Value::from(REQUEST),
            Value::from(7),
            Value::from("list_tasks"),
            Value::Array(Vec::new()),
        ]));

        let (result, notifications, output) = serve_input(request.as_slice()).await;

        result.unwrap();
        assert_eq!(notifications, Vec::<Notification>::new());
        let response = rmpv::decode::read_value(&mut output.as_slice()).unwrap();
        let fields = response.as_array().unwrap();
        assert_eq!(fields[..2], [Value::from(RESPONSE), Value::from(7)]);
        assert!(
            fields[2]
                .as_str()
                .is_some_and(|error| error.contains("rpcnotify"))
        );
        assert_eq!(fields[3], Value::Nil);
    }

    #[tokio::test]
    async fn input_ending_inside_a_message_is_an_error() {
        let message = notification_bytes("cut", 1);

        let (result, _, _) = serve_input(&message[..message.len() - 1]).await;

        assert!(matches!(result, Err(ServeError::Truncated)));
    }
}
