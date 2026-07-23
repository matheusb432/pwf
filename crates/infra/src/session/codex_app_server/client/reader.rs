use std::{
    io::{self, BufRead as _, BufReader},
    process::ChildStdout,
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
};

use serde_json::Value;

const RESPONSE_LINE_BYTES_MAX: usize = 1024 * 1024;
const RESPONSE_QUEUE_CAPACITY: usize = 4;
const RESPONSE_READER_THREAD_NAME: &str = "pwf-codex-app-server-reader";

pub(super) enum ReaderFailure {
    Transport(String),
    Malformed(String),
    ResponseLineBytesLimitExceeded { bytes_max: usize },
    Closed,
}

pub(super) fn start_response_reader(
    stdout: ChildStdout,
) -> io::Result<Receiver<Result<Value, ReaderFailure>>> {
    let (response_sender, responses) = mpsc::sync_channel(RESPONSE_QUEUE_CAPACITY);
    thread::Builder::new()
        .name(RESPONSE_READER_THREAD_NAME.to_string())
        .spawn(move || read_responses(stdout, &response_sender))?;
    Ok(responses)
}

fn read_responses(stdout: ChildStdout, response_sender: &SyncSender<Result<Value, ReaderFailure>>) {
    let mut stdout = BufReader::new(stdout);
    loop {
        let response = match read_response_line(&mut stdout) {
            Ok(Some(line)) => serde_json::from_slice(&line)
                .map_err(|error| ReaderFailure::Malformed(error.to_string())),
            Ok(None) => Err(ReaderFailure::Closed),
            Err(error) => Err(error),
        };
        let reader_failed = response.is_err();
        if response_sender.send(response).is_err() || reader_failed {
            return;
        }
    }
}

fn read_response_line(
    stdout: &mut BufReader<ChildStdout>,
) -> Result<Option<Vec<u8>>, ReaderFailure> {
    let mut line = Vec::with_capacity(RESPONSE_LINE_BYTES_MAX + 1);
    loop {
        let (bytes_consumed, newline_found) = {
            let available = stdout
                .fill_buf()
                .map_err(|error| ReaderFailure::Transport(error.to_string()))?;
            if available.is_empty() {
                return if line.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(line))
                };
            }

            let newline_index = available.iter().position(|byte| *byte == b'\n');
            let bytes_consumed =
                newline_index.map_or(available.len(), |newline_index| newline_index + 1);
            let content_length = newline_index.unwrap_or(bytes_consumed);
            let bytes_remaining = RESPONSE_LINE_BYTES_MAX + 1 - line.len();
            let bytes_copied = content_length.min(bytes_remaining);
            line.extend_from_slice(&available[..bytes_copied]);
            (bytes_consumed, newline_index.is_some())
        };
        stdout.consume(bytes_consumed);

        if line.len() > RESPONSE_LINE_BYTES_MAX {
            return Err(ReaderFailure::ResponseLineBytesLimitExceeded {
                bytes_max: RESPONSE_LINE_BYTES_MAX,
            });
        }
        if newline_found {
            return Ok(Some(line));
        }
    }
}
