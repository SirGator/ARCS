//! Geräteunabhängige Adapter-Seite des stdio-v1-Protokolls.
//!
//! Der Host prüft Grants, Schemas und Freigaben. Dieses SDK übernimmt die
//! Session und das Routing. Der Treiber bleibt für fachliche Validierung,
//! dauerhafte Observation-ACKs und idempotente Gerätewirkungen verantwortlich.

#![doc = include_str!("../README.md")]

use std::io::{BufRead, Write};

use arcs_adapter_protocol::{
    ActInvocation, ActResponse, AdapterManifest, CapabilityContract, DataChunk, DataRead,
    ManifestError, ObservationMessage, OutputInvocation, OutputResponse, RequestInvocation,
    RequestResponse, TransformInvocation, TransformResponse,
    stdio::{
        AdapterMessage, Envelope, HostMessage, WIRE_VERSION, WireError, read_frame, write_frame,
    },
};

/// Eine vom Treiber bis zum ACK aufzubewahrende Observation.
#[derive(Debug, Clone)]
pub struct PendingObservation {
    pub message_id: String,
    pub observation: ObservationMessage,
}

#[derive(Debug, Clone)]
pub struct PendingInput {
    pub message_id: String,
    pub input: arcs_adapter_protocol::InputMessage,
}

/// Ein fachlicher oder betrieblicher Fehler, der über das Protokoll gemeldet wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl AdapterError {
    pub fn rejected(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_request".into(),
            message: message.into(),
            retryable: false,
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: "unavailable".into(),
            message: message.into(),
            retryable: true,
        }
    }

    fn unsupported() -> Self {
        Self {
            code: "unsupported_operation".into(),
            message: "operation not implemented".into(),
            retryable: false,
        }
    }
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AdapterError {}

/// Geräteimplementierungen überschreiben nur die unterstützten Operationen.
///
/// Das Manifest wird beim Start einmal gelesen und validiert. Es erteilt keine
/// Berechtigungen. Eine Session darf ausschließlich von einem autorisierten
/// Host bedient werden; das SDK authentifiziert keine fremden Aufrufer.
pub trait Adapter {
    fn manifest(&self) -> AdapterManifest;
    fn reason_v2(
        &mut self,
        _invocation: arcs_adapter_protocol::ReasoningInvocationV2,
    ) -> Result<arcs_adapter_protocol::ReasoningResponseV2, AdapterError> {
        Err(AdapterError::unsupported())
    }

    fn poll_input(&mut self) -> Result<Option<PendingInput>, AdapterError> {
        Ok(None)
    }
    fn acknowledge_input(&mut self, _message_id: &str) -> Result<(), AdapterError> {
        Err(AdapterError::unsupported())
    }

    /// Liefert dieselbe ausstehende Observation bis zu deren Bestätigung.
    fn poll(&mut self) -> Result<Option<PendingObservation>, AdapterError> {
        Ok(None)
    }

    /// Muss für bereits bestätigte IDs wiederholbar sein.
    fn acknowledge(&mut self, _message_id: &str) -> Result<(), AdapterError> {
        Err(AdapterError::unsupported())
    }

    fn request(&mut self, _invocation: RequestInvocation) -> Result<RequestResponse, AdapterError> {
        Err(AdapterError::unsupported())
    }

    /// Liefert einen vom Host geprüften und freigegebenen Ausgabeauftrag.
    /// Identische Invocation-IDs benötigen dauerhafte Idempotenz im Treiber.
    fn output(&mut self, _invocation: OutputInvocation) -> Result<OutputResponse, AdapterError> {
        Err(AdapterError::unsupported())
    }

    /// Reine Verarbeitung: dieselbe Eingabe unter derselben ID muss dasselbe
    /// Ergebnis liefern. Der Adapter validiert zusätzlich seine fachlichen Daten.
    fn transform(
        &mut self,
        _invocation: TransformInvocation,
    ) -> Result<TransformResponse, AdapterError> {
        Err(AdapterError::unsupported())
    }

    /// Der Treiber prüft den fachlichen Payload und garantiert seine Idempotenz.
    fn execute(&mut self, _invocation: ActInvocation) -> Result<ActResponse, AdapterError> {
        Err(AdapterError::unsupported())
    }

    /// Liest unveränderliche Daten ausschließlich anhand einer angebotenen Referenz.
    fn read_data(&mut self, _request: DataRead) -> Result<DataChunk, AdapterError> {
        Err(AdapterError::unsupported())
    }
}

#[derive(Debug)]
pub enum ServerError {
    Manifest(ManifestError),
    Wire(WireError),
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "adapter server: {self:?}")
    }
}

impl std::error::Error for ServerError {}

/// Bedient eine synchrone Session bis `shutdown` oder einem Transportfehler.
///
/// Fachliche Fehler werden korreliert zurückgegeben. Ungültige Frames beenden
/// die Session. Es gibt keine impliziten Retries oder automatische ACKs.
pub fn serve(
    adapter: &mut impl Adapter,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> Result<(), ServerError> {
    let manifest = adapter.manifest();
    manifest.validate().map_err(ServerError::Manifest)?;
    let mut registered = false;
    loop {
        let envelope: Envelope<HostMessage> = read_frame(reader).map_err(ServerError::Wire)?;
        let shutdown = envelope.protocol_version == WIRE_VERSION
            && matches!(envelope.message, HostMessage::Shutdown);
        let result = if envelope.protocol_version != WIRE_VERSION {
            Err(AdapterError {
                code: "unsupported_version".into(),
                message: "expected protocol 1".into(),
                retryable: false,
            })
        } else {
            match envelope.message {
                HostMessage::Shutdown => Ok(AdapterMessage::Bye),
                HostMessage::Hello if !registered => {
                    registered = true;
                    Ok(AdapterMessage::Manifest {
                        manifest: manifest.clone(),
                    })
                }
                message if registered => dispatch(adapter, &manifest, message),
                _ => Err(AdapterError {
                    code: "invalid_state".into(),
                    message: "hello required before operations".into(),
                    retryable: false,
                }),
            }
        };
        let message = result.unwrap_or_else(|error| AdapterMessage::Error {
            code: error.code,
            message: error.message,
            retryable: error.retryable,
        });
        write_frame(
            writer,
            &Envelope {
                protocol_version: WIRE_VERSION,
                request_id: envelope.request_id,
                message,
            },
        )
        .map_err(ServerError::Wire)?;
        if shutdown {
            return Ok(());
        }
    }
}

fn invalid_response(message: &str) -> AdapterError {
    AdapterError {
        code: "invalid_response".into(),
        message: message.into(),
        retryable: false,
    }
}

fn dispatch(
    adapter: &mut impl Adapter,
    manifest: &AdapterManifest,
    message: HostMessage,
) -> Result<AdapterMessage, AdapterError> {
    match message {
        HostMessage::ReasonV2 { invocation } => {
            if invocation.capability.adapter_id!=manifest.adapter_id || invocation.invocation_id.trim().is_empty()
                || invocation.request_id.trim().is_empty() || !invocation.budget.supports(invocation.method)
                || !manifest.capability(&invocation.capability.capability_id).is_some_and(|cap|matches!(&cap.contract,CapabilityContract::Reason {emits} if emits.contains(&invocation.target_schema_id)))
            {return Err(AdapterError::rejected("reasoning contract mismatch"));}
            let id = invocation.invocation_id.clone();
            let request = invocation.request_id.clone();
            let method = invocation.method;
            let budget = invocation.budget.clone();
            let response = adapter.reason_v2(invocation)?;
            if response.invocation_id != id
                || response.request_id != request
                || response.trace.method != method
                || response.candidates.len() > budget.max_candidates
                || serde_json::to_vec(&response)
                    .map_err(|_| invalid_response("invalid reasoning response"))?
                    .len()
                    > budget.max_output_bytes
            {
                return Err(invalid_response("reasoning correlation or size mismatch"));
            }
            Ok(AdapterMessage::ReasoningResultV2 { response })
        }
        HostMessage::PollInput => match adapter.poll_input()? {
            None => Ok(AdapterMessage::Idle),
            Some(pending) => {
                if pending.message_id.trim().is_empty()
                    || pending.message_id.len() > 128
                    || !manifest
                        .capability(&pending.input.capability_id)
                        .is_some_and(|cap| cap.contract.is_input())
                {
                    return Err(invalid_response("invalid input identity or capability"));
                }
                Ok(AdapterMessage::Input {
                    message_id: pending.message_id,
                    input: pending.input,
                })
            }
        },
        HostMessage::AckInput { message_id } => {
            if message_id.trim().is_empty() || message_id.len() > 128 {
                return Err(AdapterError::rejected("invalid input ACK"));
            }
            adapter.acknowledge_input(&message_id)?;
            Ok(AdapterMessage::InputAcknowledged { message_id })
        }
        HostMessage::Output { invocation } => {
            let contract = manifest
                .capability(&invocation.capability.capability_id)
                .map(|cap| &cap.contract);
            if invocation.capability.adapter_id != manifest.adapter_id
                || invocation.invocation_id.trim().is_empty()
                || invocation.request_version_id.0.trim().is_empty()
                || invocation.approval_version_id.0.trim().is_empty()
                || invocation.artifact_version_id.0.trim().is_empty()
                || !matches!(contract,Some(CapabilityContract::Output { accepts, emits, idempotent:true })
                    if accepts.contains(&invocation.artifact_schema_id) && emits.contains(&invocation.result_schema_id))
            {
                return Err(AdapterError::rejected("output contract mismatch"));
            }
            let id = invocation.invocation_id.clone();
            let maximum = invocation.max_result_bytes;
            let response = adapter.output(invocation)?;
            if response.invocation_id != id
                || response.external_reference.trim().is_empty()
                || serde_json::to_vec(&response.result_payload)
                    .map_err(|_| invalid_response("invalid output receipt"))?
                    .len()
                    > maximum
            {
                return Err(invalid_response(
                    "output correlation, reference or result limit mismatch",
                ));
            }
            Ok(AdapterMessage::OutputResult { response })
        }
        HostMessage::Transform { invocation } => {
            let contract = manifest
                .capability(&invocation.capability.capability_id)
                .map(|cap| &cap.contract);
            if invocation.capability.adapter_id != manifest.adapter_id
                || invocation.invocation_id.trim().is_empty()
                || invocation.input_version_id.0.trim().is_empty()
                || !matches!(contract, Some(CapabilityContract::Transform { accepts, emits })
                    if accepts.contains(&invocation.input_schema_id) && emits.contains(&invocation.output_schema_id))
            {
                return Err(AdapterError::rejected("transform contract mismatch"));
            }
            let id = invocation.invocation_id.clone();
            let limit = invocation.max_output_bytes;
            let response = adapter.transform(invocation)?;
            if response.invocation_id != id
                || response.external_reference.trim().is_empty()
                || serde_json::to_vec(&response.payload)
                    .map_err(|_| invalid_response("invalid transform payload"))?
                    .len()
                    > limit
            {
                return Err(invalid_response(
                    "transform correlation, reference or payload limit mismatch",
                ));
            }
            Ok(AdapterMessage::TransformResult { response })
        }
        HostMessage::ReadData { request } => {
            if !request.is_valid()
                || request.capability.adapter_id != manifest.adapter_id
                || !manifest
                    .capability(&request.capability.capability_id)
                    .is_some_and(|cap| {
                        cap.contract.is_observation()
                            || cap.contract.is_request()
                            || cap.contract.is_transform()
                    })
            {
                return Err(AdapterError::rejected(
                    "data reference or capability mismatch",
                ));
            }
            let chunk = adapter.read_data(request.clone())?;
            if !chunk.matches(&request) {
                return Err(invalid_response(
                    "data chunk identity, offset or length mismatch",
                ));
            }
            Ok(AdapterMessage::DataChunk { chunk })
        }
        HostMessage::Poll => match adapter.poll()? {
            None => Ok(AdapterMessage::Idle),
            Some(pending) => {
                if pending.message_id.trim().is_empty()
                    || pending.message_id.len() > 128
                    || !manifest
                        .capability(&pending.observation.capability_id)
                        .is_some_and(|cap| cap.contract.is_observation())
                {
                    return Err(invalid_response(
                        "invalid observation identity or capability",
                    ));
                }
                Ok(AdapterMessage::Observation {
                    message_id: pending.message_id,
                    observation: pending.observation,
                })
            }
        },
        HostMessage::Ack { message_id } => {
            if message_id.trim().is_empty() || message_id.len() > 128 {
                return Err(AdapterError::rejected("invalid acknowledgement ID"));
            }
            adapter.acknowledge(&message_id)?;
            Ok(AdapterMessage::Acknowledged { message_id })
        }
        HostMessage::Request { invocation } => {
            let contract = manifest
                .capability(&invocation.capability.capability_id)
                .map(|cap| &cap.contract);
            if invocation.capability.adapter_id != manifest.adapter_id
                || invocation.invocation_id.trim().is_empty()
                || !matches!(contract, Some(CapabilityContract::Request { accepts, emits })
                    if accepts.contains(&invocation.request_schema_id) && emits.contains(&invocation.response_schema_id))
            {
                return Err(AdapterError::rejected("request contract mismatch"));
            }
            let id = invocation.invocation_id.clone();
            let limit = invocation.max_response_bytes;
            let response = adapter.request(invocation)?;
            if response.invocation_id != id
                || response.external_reference.trim().is_empty()
                || serde_json::to_vec(&response.payload)
                    .map_err(|_| invalid_response("invalid request payload"))?
                    .len()
                    > limit
            {
                return Err(invalid_response(
                    "request correlation, reference or payload limit mismatch",
                ));
            }
            Ok(AdapterMessage::RequestResult { response })
        }
        HostMessage::Execute { invocation } => {
            let contract = manifest
                .capability(&invocation.capability.capability_id)
                .map(|cap| &cap.contract);
            if invocation.capability.adapter_id != manifest.adapter_id
                || invocation.invocation_id.trim().is_empty()
                || invocation.action_version_id.0.trim().is_empty()
                || !matches!(contract, Some(CapabilityContract::Act { emits, .. })
                    if emits.contains(&invocation.result_schema_id))
            {
                return Err(AdapterError::rejected("action contract mismatch"));
            }
            let id = invocation.invocation_id.clone();
            let response = adapter.execute(invocation)?;
            if response.invocation_id != id || response.external_reference.trim().is_empty() {
                return Err(invalid_response("action correlation or reference mismatch"));
            }
            Ok(AdapterMessage::ExecutionResult { response })
        }
        _ => Err(AdapterError {
            code: "invalid_state".into(),
            message: "hello already completed".into(),
            retryable: false,
        }),
    }
}
