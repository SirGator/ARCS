//! Bounded local control over the monitor's own standard input.
use std::io::{self, BufRead, Read, Write};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread;

use serde::Deserialize;
use serde_json::{Value, json};

use super::emit;
use crate::task_runtime::queue::MAX_TASK_QUEUE_PAGE;
use crate::{Host, operator_command::OperatorCommand, task_runtime::TaskRuntime};

const MAX_LINE_BYTES: usize = 65_536;
const QUEUE_SIZE: usize = 16;
const COMMANDS_PER_TICK: usize = 8;

enum Frame {
    Line(Vec<u8>),
    Invalid(&'static str),
}

pub struct MonitorControl {
    frames: Receiver<Frame>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    request_id: String,
    control: OperatorCommand,
}

#[derive(Deserialize)]
struct Correlation {
    request_id: String,
}

fn valid_request_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

impl MonitorControl {
    /// The reader belongs to the CLI process. A blocked stdin read never delays shutdown.
    pub fn from_stdin() -> io::Result<Self> {
        let (sender, frames) = sync_channel(QUEUE_SIZE);
        thread::Builder::new()
            .name("arcs-monitor-control".into())
            .spawn(move || {
                if read_frames(io::stdin().lock(), &sender).is_err() {
                    let _ = sender.send(Frame::Invalid("failed to read monitor control input"));
                }
            })?;
        Ok(Self { frames })
    }

    pub(super) fn description(&self) -> Value {
        json!({"transport":"stdin_jsonl","protocol_version":1,"max_line_bytes":MAX_LINE_BYTES,"commands_per_tick":COMMANDS_PER_TICK,"goal_review_preview":true,"task_queue":{"max_page_size":MAX_TASK_QUEUE_PAGE}})
    }

    pub(super) fn apply_pending(
        &self,
        host: &mut Host,
        runtime: &mut TaskRuntime,
        writer: &mut impl Write,
    ) -> Result<bool, String> {
        let mut processed = false;
        for frame in self.frames.try_iter().take(COMMANDS_PER_TICK) {
            emit(writer, respond(frame, host, runtime))?;
            processed = true;
        }
        Ok(processed)
    }
}

fn read_frames(mut reader: impl BufRead, sender: &SyncSender<Frame>) -> io::Result<()> {
    loop {
        let mut line = Vec::new();
        let size = (&mut reader)
            .take((MAX_LINE_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if size == 0 {
            return Ok(());
        }
        let (frame, discard_rest, eof) = if line.last() == Some(&b'\n') {
            line.pop();
            (Frame::Line(line), false, false)
        } else if size > MAX_LINE_BYTES {
            (
                Frame::Invalid("monitor control line exceeds 65536 bytes"),
                true,
                false,
            )
        } else {
            (
                Frame::Invalid("monitor control line ended without a newline"),
                false,
                true,
            )
        };
        if sender.send(frame).is_err() || eof {
            return Ok(());
        }
        if discard_rest {
            reader.skip_until(b'\n')?;
        }
    }
}

fn respond(frame: Frame, host: &mut Host, runtime: &mut TaskRuntime) -> Value {
    let line = match frame {
        Frame::Line(line) => line,
        Frame::Invalid(message) => return failure(None, "invalid_frame", message),
    };
    let request: Request = match serde_json::from_slice(&line) {
        Ok(request) => request,
        Err(error) => {
            let correlation = serde_json::from_slice::<Correlation>(&line)
                .ok()
                .filter(|value| valid_request_id(&value.request_id));
            return failure(
                correlation.as_ref().map(|value| value.request_id.as_str()),
                "invalid_request",
                &error.to_string(),
            );
        }
    };
    if !valid_request_id(&request.request_id) {
        return failure(None, "invalid_request", "invalid monitor request ID");
    }
    match request.control.apply(runtime, host) {
        Ok(receipt) => {
            json!({"event":"control_result","request_id":request.request_id,"ok":true,"receipt":receipt})
        }
        Err(error) => failure(
            Some(&request.request_id),
            "operation_failed",
            &error.to_string(),
        ),
    }
}

fn failure(request_id: Option<&str>, code: &str, message: &str) -> Value {
    let message: String = message.chars().take(1000).collect();
    json!({"event":"control_result","request_id":request_id,"ok":false,"error":{"code":code,"message":message}})
}
