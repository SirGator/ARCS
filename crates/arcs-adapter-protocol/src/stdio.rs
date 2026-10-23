//! JSON-Lines-Vertrag für Observations, Requests, Transforms, Actions und Datenabruf.

use std::io::{BufRead, Read, Write};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    ActInvocation, ActResponse, AdapterManifest, DataChunk, DataRead, ObservationMessage,
    OutputInvocation, OutputResponse, RequestInvocation, RequestResponse, TransformInvocation,
    TransformResponse,
};

pub const WIRE_VERSION: u32 = 1;
/// Einschließlich des abschließenden Newlines; gilt in beiden Richtungen.
pub const MAX_FRAME_BYTES: usize = 65_536;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope<T> {
    pub protocol_version: u32,
    pub request_id: String,
    pub message: T,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostMessage {
    Hello,
    Poll,
    Ack { message_id: String },
    Request { invocation: RequestInvocation },
    Transform { invocation: TransformInvocation },
    Output { invocation: OutputInvocation },
    Execute { invocation: ActInvocation },
    ReadData { request: DataRead },
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AdapterMessage {
    Manifest {
        manifest: AdapterManifest,
    },
    Observation {
        message_id: String,
        observation: ObservationMessage,
    },
    Idle,
    Acknowledged {
        message_id: String,
    },
    RequestResult {
        response: RequestResponse,
    },
    TransformResult {
        response: TransformResponse,
    },
    OutputResult {
        response: OutputResponse,
    },
    ExecutionResult {
        response: ActResponse,
    },
    DataChunk {
        chunk: DataChunk,
    },
    Error {
        code: String,
        message: String,
        retryable: bool,
    },
    Bye,
}

#[derive(Debug)]
pub enum WireError {
    Io(std::io::Error),
    Json(serde_json::Error),
    FrameTooLarge,
    Disconnected,
}

/// Liest höchstens ein Frame plus ein Byte, auch bei fehlendem Newline.
pub fn read_frame<T: DeserializeOwned>(reader: &mut impl BufRead) -> Result<T, WireError> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_FRAME_BYTES as u64 + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(WireError::Io)?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(WireError::FrameTooLarge);
    }
    if bytes.last() != Some(&b'\n') {
        return Err(WireError::Disconnected);
    }
    serde_json::from_slice(&bytes).map_err(WireError::Json)
}

pub fn write_frame<T: Serialize>(writer: &mut impl Write, message: &T) -> Result<(), WireError> {
    let mut bytes = serde_json::to_vec(message).map_err(WireError::Json)?;
    bytes.push(b'\n');
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(WireError::FrameTooLarge);
    }
    writer.write_all(&bytes).map_err(WireError::Io)?;
    writer.flush().map_err(WireError::Io)
}
