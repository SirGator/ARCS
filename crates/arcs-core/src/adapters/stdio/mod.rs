//! Vom Betreiber gestarteter Child-Prozess, gebunden an genau einen Grant.
//! stdio ist eine lokale Prozessgrenze für alle sieben Rollen, einschließlich
//! Input-ACKs und Reasoning V2; Netzwerk-Authentifizierung ist nicht enthalten.

mod ingress;
pub mod protocol;

pub use ingress::{
    CommittedIngress, IngressAcknowledgement, IngressKind, IngressMessage, IngressReply,
};

use std::io::BufReader;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::adapters::data::{DataAdapter, DataChunk, DataRead};
use crate::adapters::{
    ActAdapter, ActFence, ActInvocation, ActResponse, AdapterCallError, AdapterGrant,
    AdapterManifest, AdapterRegistry, AdapterRegistryError, CapabilityRef,
};
use crate::core::SchemaRegistry;
use crate::output::{OutputAdapter, OutputInvocation, OutputResponse};
use crate::request::{RequestAdapter, RequestInvocation, RequestResponse};
use crate::store::SqliteArtifactStore;
use crate::transform::{TransformAdapter, TransformInvocation, TransformResponse};
use crate::world::observation::{ObservationError, ObservationService, RecordedObservation};

use protocol::{
    AdapterMessage, Envelope, HostMessage, WIRE_VERSION, WireError, read_frame, write_frame,
};

#[derive(Debug)]
pub enum StdioError {
    Spawn(std::io::Error),
    Wire(WireError),
    Timeout,
    Disconnected,
    UnsupportedVersion(u32),
    CorrelationMismatch,
    UnexpectedMessage,
    ExchangeInProgress,
    NoIngressExchange,
    Remote {
        code: String,
        message: String,
        retryable: bool,
    },
    Registration(AdapterRegistryError),
    RegistrationChanged,
    SessionPoisoned,
    Observation(ObservationError),
    Input(crate::input::InputError),
}

type Response = (Instant, Result<Envelope<AdapterMessage>, WireError>);

/// Ein serieller Dialog. Nach Timeout/Protokollfehler wird das Child beendet;
/// verspätete Antworten können keinen späteren Aufruf erfüllen.
pub struct StdioAdapter {
    manifest: AdapterManifest,
    session: Mutex<Session>,
    child: Arc<Mutex<Child>>,
    cancelled: Arc<AtomicBool>,
}

struct Session {
    child: Arc<Mutex<Child>>,
    cancelled: Arc<AtomicBool>,
    requests: Option<SyncSender<Envelope<HostMessage>>>,
    responses: Receiver<Response>,
    worker: Option<JoinHandle<()>>,
    timeout: Duration,
    next_request: u64,
    ingress: Option<ingress::PendingIngress>,
}

impl StdioAdapter {
    pub fn poll_input(
        &self,
        inputs: &mut crate::input::InputService<'_>,
    ) -> Result<Option<crate::core::Artifact>, StdioError> {
        match self.exchange(HostMessage::PollInput)? {
            AdapterMessage::Idle => Ok(None),
            AdapterMessage::Input { message_id, input } => {
                let artifact = inputs
                    .ingest_once(&self.manifest.adapter_id, &message_id, input)
                    .map_err(|error| {
                        self.stop();
                        StdioError::Input(error)
                    })?;
                match self.exchange(HostMessage::AckInput {
                    message_id: message_id.clone(),
                })? {
                    AdapterMessage::InputAcknowledged { message_id: ack } if ack == message_id => {
                        Ok(Some(artifact))
                    }
                    _ => {
                        self.stop();
                        Err(StdioError::UnexpectedMessage)
                    }
                }
            }
            _ => {
                self.stop();
                Err(StdioError::UnexpectedMessage)
            }
        }
    }
    pub fn connect(
        command: &mut Command,
        timeout: Duration,
        grant: AdapterGrant,
        registry: &mut AdapterRegistry,
        schemas: &SchemaRegistry,
        store: &SqliteArtifactStore,
    ) -> Result<Self, StdioError> {
        let endpoint = Self::handshake(command, timeout)?;
        let manifest = endpoint.manifest();
        if let Some(installed) = registry.get(&grant.adapter_id) {
            if installed.manifest() != manifest || installed.grant() != &grant {
                return Err(StdioError::RegistrationChanged);
            }
            store
                .bind_schemas(schemas)
                .map_err(|error| StdioError::Registration(AdapterRegistryError::Store(error)))?;
        } else {
            registry
                .register(manifest.clone(), grant, schemas, store)
                .map_err(StdioError::Registration)?;
        }
        Ok(endpoint)
    }

    /// Transport-only handshake. No registry changes, Store access or capability
    /// invocation. The owning Core must validate the result before installing it.
    pub fn handshake(command: &mut Command, timeout: Duration) -> Result<Self, StdioError> {
        Self::handshake_cancellable(command, timeout, Arc::new(AtomicBool::new(false)))
    }

    /// Cancellation is observed at most every 25ms while waiting for metadata.
    pub fn handshake_cancellable(
        command: &mut Command,
        timeout: Duration,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, StdioError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(StdioError::Disconnected);
        }
        // Kein Shell-String und keine vom Child frei wählbare Identität.
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(StdioError::Spawn)?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (requests, incoming) = mpsc::sync_channel(1);
        let (outgoing, responses) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("arcs-stdio".into())
            .spawn(move || {
                let mut stdout = BufReader::new(stdout);
                while let Ok(request) = incoming.recv() {
                    let response =
                        write_frame(&mut stdin, &request).and_then(|()| read_frame(&mut stdout));
                    let failed = response.is_err();
                    if outgoing.send((Instant::now(), response)).is_err() || failed {
                        break;
                    }
                }
            });
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(StdioError::Spawn(error));
            }
        };
        let child = Arc::new(Mutex::new(child));
        let mut session = Session {
            child: Arc::clone(&child),
            cancelled: Arc::clone(&cancelled),
            requests: Some(requests),
            responses,
            worker: Some(worker),
            timeout,
            next_request: 1,
            ingress: None,
        };
        let AdapterMessage::Manifest { manifest } = session.exchange(HostMessage::Hello)? else {
            return Err(StdioError::UnexpectedMessage);
        };
        Ok(Self {
            manifest,
            session: Mutex::new(session),
            child,
            cancelled,
        })
    }

    pub fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    /// Change only the local dialog deadline, without I/O or registration.
    /// Reconnect workers restore the configured RPC timeout after their
    /// separately bounded metadata handshake.
    pub fn set_dialog_timeout(&mut self, timeout: Duration) -> Result<(), StdioError> {
        self.session
            .get_mut()
            .map_err(|_| StdioError::SessionPoisoned)?
            .timeout = timeout;
        Ok(())
    }

    /// Poll liefert höchstens eine Observation. Idle beendet keinen Prozess.
    /// Wiederholte Messungen liefern den ursprünglichen Commit zurück.
    pub fn poll(
        &self,
        observations: &mut ObservationService<'_>,
    ) -> Result<Option<RecordedObservation>, StdioError> {
        match self.exchange(HostMessage::Poll)? {
            AdapterMessage::Observation {
                message_id,
                observation,
            } => {
                let recorded = match observations.ingest_once(
                    &self.manifest.adapter_id,
                    &message_id,
                    observation,
                ) {
                    Ok(recorded) => recorded,
                    Err(error) => {
                        self.stop();
                        return Err(StdioError::Observation(error));
                    }
                };
                // Bestätigung ausschließlich nach erfolgreichem Store-Commit.
                match self.exchange(HostMessage::Ack {
                    message_id: message_id.clone(),
                })? {
                    AdapterMessage::Acknowledged {
                        message_id: acknowledged,
                    } if acknowledged == message_id => Ok(Some(recorded)),
                    _ => {
                        self.stop();
                        Err(StdioError::UnexpectedMessage)
                    }
                }
            }
            AdapterMessage::Idle => Ok(None),
            _ => {
                self.stop();
                Err(StdioError::UnexpectedMessage)
            }
        }
    }

    pub fn shutdown(&self) -> Result<(), StdioError> {
        let result = match self.exchange(HostMessage::Shutdown) {
            Ok(AdapterMessage::Bye) => Ok(()),
            Ok(_) => Err(StdioError::UnexpectedMessage),
            Err(error) => Err(error),
        };
        self.stop();
        result
    }

    /// Bricht ausschließlich den eigenen Adapterprozess ab, ohne auf den
    /// Session-Mutex eines laufenden Dialogs zu warten. Externe Wirkungen
    /// können dabei bereits eingetreten sein und bleiben ein Recovery-Fall.
    pub fn abort(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }

    fn exchange(&self, message: HostMessage) -> Result<AdapterMessage, StdioError> {
        self.session
            .lock()
            .map_err(|_| StdioError::SessionPoisoned)?
            .exchange(message)
    }

    fn stop(&self) {
        if let Ok(mut session) = self.session.lock() {
            session.stop();
        }
    }

    fn protocol_error(&self, detail: &str) -> AdapterCallError {
        self.stop();
        AdapterCallError::Protocol(detail.into())
    }
}

impl crate::reasoning::general::GeneralReasoningAdapter for StdioAdapter {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }
    fn propose_v2(
        &self,
        invocation: &crate::reasoning::ReasoningInvocationV2,
    ) -> Result<crate::reasoning::ReasoningResponseV2, AdapterCallError> {
        self.propose_with_data(invocation, &[])
    }
    fn propose_with_data(
        &self,
        invocation: &crate::reasoning::ReasoningInvocationV2,
        inputs: &[arcs_adapter_protocol::ReceivedInputData],
    ) -> Result<crate::reasoning::ReasoningResponseV2, AdapterCallError> {
        if invocation.capability.adapter_id != self.manifest.adapter_id
            || inputs.len() != invocation.input_data.len()
            || inputs
                .iter()
                .zip(&invocation.input_data)
                .any(|(input, descriptor)| {
                    &input.descriptor != descriptor
                        || input.bytes.len() as u64 != descriptor.reference.size_bytes
                })
        {
            return Err(self.protocol_error("reasoning endpoint or binary inputs mismatch"));
        }
        let mut session = self
            .session
            .lock()
            .map_err(|_| AdapterCallError::Unavailable("session poisoned".into()))?;
        let previous = session.timeout;
        let started = std::time::Instant::now();
        let budget = Duration::from_millis(invocation.budget.max_runtime_ms);
        let exchange =
            |session: &mut Session, message: HostMessage| -> Result<AdapterMessage, StdioError> {
                let Some(remaining) = budget
                    .checked_sub(started.elapsed())
                    .filter(|duration| !duration.is_zero())
                else {
                    return Err(StdioError::Timeout);
                };
                session.timeout = previous.min(remaining);
                session.exchange(message)
            };
        let result = (|| {
            if !inputs.is_empty() {
                match exchange(
                    &mut session,
                    HostMessage::BeginInputData {
                        invocation_id: invocation.invocation_id.clone(),
                        target: invocation.capability.clone(),
                        inputs: invocation.input_data.clone(),
                    },
                )? {
                    AdapterMessage::InputDataReady { invocation_id }
                        if invocation_id == invocation.invocation_id => {}
                    _ => return Err(StdioError::UnexpectedMessage),
                }
                for input in inputs {
                    let mut offset = 0u64;
                    for bytes in input
                        .bytes
                        .chunks(arcs_adapter_protocol::MAX_DATA_CHUNK_BYTES as usize)
                    {
                        let next_offset = offset + bytes.len() as u64;
                        match exchange(
                            &mut session,
                            HostMessage::InputDataChunk {
                                invocation_id: invocation.invocation_id.clone(),
                                name: input.descriptor.selection.name.clone(),
                                offset,
                                bytes: bytes.to_vec(),
                            },
                        )? {
                            AdapterMessage::InputDataStored {
                                invocation_id,
                                name,
                                next_offset: ack,
                            } if invocation_id == invocation.invocation_id
                                && name == input.descriptor.selection.name
                                && ack == next_offset => {}
                            _ => return Err(StdioError::UnexpectedMessage),
                        }
                        offset = next_offset;
                    }
                }
            }
            exchange(
                &mut session,
                HostMessage::ReasonV2 {
                    invocation: invocation.clone(),
                },
            )
        })();
        session.timeout = previous;
        if result.is_err() {
            session.stop();
        }
        drop(session);
        match result.map_err(call_error)? {
            AdapterMessage::ReasoningResultV2 { response }
                if response.invocation_id == invocation.invocation_id
                    && response.request_id == invocation.request_id =>
            {
                Ok(response)
            }
            _ => Err(self.protocol_error("invalid reasoning response correlation")),
        }
    }
}

impl DataAdapter for StdioAdapter {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn read_chunk(&self, request: &DataRead) -> Result<DataChunk, AdapterCallError> {
        if request.capability.adapter_id != self.manifest.adapter_id || !request.is_valid() {
            return Err(AdapterCallError::Protocol(
                "data endpoint or reference mismatch".into(),
            ));
        }
        match self
            .exchange(HostMessage::ReadData {
                request: request.clone(),
            })
            .map_err(call_error)?
        {
            AdapterMessage::DataChunk { chunk } if chunk.matches(request) => Ok(chunk),
            _ => Err(self.protocol_error("invalid data chunk type, identity, offset or length")),
        }
    }
}

impl OutputAdapter for StdioAdapter {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }
    fn deliver(&self, invocation: &OutputInvocation) -> Result<OutputResponse, AdapterCallError> {
        if invocation.capability.adapter_id != self.manifest.adapter_id {
            return Err(AdapterCallError::Protocol(
                "output endpoint identity mismatch".into(),
            ));
        }
        match self
            .exchange(HostMessage::Output {
                invocation: invocation.clone(),
            })
            .map_err(call_error)?
        {
            AdapterMessage::OutputResult { response }
                if response.invocation_id == invocation.invocation_id =>
            {
                Ok(response)
            }
            _ => Err(self.protocol_error("invalid output response type or invocation ID")),
        }
    }
}

impl TransformAdapter for StdioAdapter {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn transform(
        &self,
        invocation: &TransformInvocation,
    ) -> Result<TransformResponse, AdapterCallError> {
        if invocation.capability.adapter_id != self.manifest.adapter_id {
            return Err(AdapterCallError::Protocol(
                "transform endpoint identity mismatch".into(),
            ));
        }
        match self
            .exchange(HostMessage::Transform {
                invocation: invocation.clone(),
            })
            .map_err(call_error)?
        {
            AdapterMessage::TransformResult { response }
                if response.invocation_id == invocation.invocation_id =>
            {
                Ok(response)
            }
            _ => Err(self.protocol_error("invalid transform result type or invocation ID")),
        }
    }
}

impl RequestAdapter for StdioAdapter {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn fetch(&self, invocation: &RequestInvocation) -> Result<RequestResponse, AdapterCallError> {
        if invocation.capability.adapter_id != self.manifest.adapter_id {
            return Err(AdapterCallError::Protocol(
                "request endpoint identity mismatch".into(),
            ));
        }
        match self
            .exchange(HostMessage::Request {
                invocation: invocation.clone(),
            })
            .map_err(call_error)?
        {
            AdapterMessage::RequestResult { response }
                if response.invocation_id == invocation.invocation_id =>
            {
                Ok(response)
            }
            _ => Err(self.protocol_error("invalid request response type or invocation ID")),
        }
    }
}

impl StdioAdapter {
    fn dispatch_action(
        &self,
        invocation: &ActInvocation,
        message: HostMessage,
    ) -> Result<ActResponse, AdapterCallError> {
        if invocation.capability.adapter_id != self.manifest.adapter_id {
            return Err(AdapterCallError::Protocol(
                "action endpoint identity mismatch".into(),
            ));
        }
        match self.exchange(message).map_err(call_error)? {
            AdapterMessage::ExecutionResult { response }
                if response.invocation_id == invocation.invocation_id =>
            {
                Ok(response)
            }
            _ => Err(self.protocol_error("invalid execution response type or invocation ID")),
        }
    }
}

impl ActAdapter for StdioAdapter {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn execute(&self, invocation: &ActInvocation) -> Result<ActResponse, AdapterCallError> {
        self.dispatch_action(
            invocation,
            HostMessage::Execute {
                invocation: invocation.clone(),
            },
        )
    }

    fn execute_fenced(
        &self,
        invocation: &ActInvocation,
        fence: &ActFence,
    ) -> Result<ActResponse, AdapterCallError> {
        self.dispatch_action(
            invocation,
            HostMessage::ExecuteFenced {
                invocation: invocation.clone(),
                fence: fence.clone(),
            },
        )
    }
    fn close_fence(
        &self,
        capability: &CapabilityRef,
        fence: &ActFence,
    ) -> Result<(), AdapterCallError> {
        if capability.adapter_id != self.manifest.adapter_id {
            return Err(AdapterCallError::Protocol(
                "controller endpoint identity mismatch".into(),
            ));
        }
        match self
            .exchange(HostMessage::CloseFence {
                capability: capability.clone(),
                fence: fence.clone(),
            })
            .map_err(call_error)?
        {
            AdapterMessage::FenceClosed {
                capability: actual,
                fence: closed,
            } if &actual == capability && &closed == fence => Ok(()),
            _ => Err(self.protocol_error("controller did not confirm the exact reservation fence")),
        }
    }
}

fn call_error(error: StdioError) -> AdapterCallError {
    match error {
        StdioError::Timeout => AdapterCallError::Timeout,
        StdioError::Disconnected | StdioError::Wire(WireError::Disconnected | WireError::Io(_)) => {
            AdapterCallError::Unavailable(format!("{error:?}"))
        }
        StdioError::Remote {
            code,
            message,
            retryable: true,
        } => AdapterCallError::Unavailable(format!("{code}: {message}")),
        StdioError::Remote {
            code,
            message,
            retryable: false,
        } => AdapterCallError::Rejected(format!("{code}: {message}")),
        _ => AdapterCallError::Protocol(format!("{error:?}")),
    }
}

impl Session {
    fn exchange(&mut self, message: HostMessage) -> Result<AdapterMessage, StdioError> {
        if self.ingress.is_some() {
            return Err(StdioError::ExchangeInProgress);
        }
        let closes_fence = matches!(message, HostMessage::CloseFence { .. });
        let result = self.exchange_inner(message);
        // A correlated controller refusal leaves the resource held and can
        // be retried. Transport failures still retire the session.
        if result
            .as_ref()
            .is_err_and(|error| !(closes_fence && matches!(error, StdioError::Remote { .. })))
        {
            self.stop();
        }
        result
    }

    fn send(&mut self, message: HostMessage) -> Result<String, StdioError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(StdioError::Disconnected);
        }
        let request_id = format!("stdio-{}", self.next_request);
        self.next_request = self
            .next_request
            .checked_add(1)
            .ok_or(StdioError::Disconnected)?;
        self.requests
            .as_ref()
            .ok_or(StdioError::Disconnected)?
            .try_send(Envelope {
                protocol_version: WIRE_VERSION,
                request_id: request_id.clone(),
                message,
            })
            .map_err(|_| StdioError::Disconnected)?;
        Ok(request_id)
    }

    fn exchange_inner(&mut self, message: HostMessage) -> Result<AdapterMessage, StdioError> {
        let request_id = self.send(message)?;
        let started = Instant::now();
        let response = loop {
            if self.cancelled.load(Ordering::Acquire) {
                return Err(StdioError::Disconnected);
            }
            let remaining = self
                .timeout
                .checked_sub(started.elapsed())
                .filter(|remaining| !remaining.is_zero())
                .ok_or(StdioError::Timeout)?;
            match self
                .responses
                .recv_timeout(remaining.min(Duration::from_millis(25)))
            {
                Ok((_, response)) => break response.map_err(StdioError::Wire)?,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Err(StdioError::Disconnected),
            }
        };
        Self::correlate(&request_id, response)
    }

    fn correlate(
        request_id: &str,
        response: Envelope<AdapterMessage>,
    ) -> Result<AdapterMessage, StdioError> {
        if response.protocol_version != WIRE_VERSION {
            return Err(StdioError::UnsupportedVersion(response.protocol_version));
        }
        if response.request_id != request_id {
            return Err(StdioError::CorrelationMismatch);
        }
        match response.message {
            AdapterMessage::Error {
                code,
                message,
                retryable,
            } => Err(StdioError::Remote {
                code,
                message,
                retryable,
            }),
            message => Ok(message),
        }
    }

    fn stop(&mut self) {
        self.ingress = None;
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.requests.take();
        if let Some(worker) = self.worker.take() {
            // Ein vom Child gestarteter Nachfahre könnte stdout weiter offen
            // halten. Der Host darf dadurch seine Deadline nicht verlieren.
            // Das direkte Child wird oben immer beendet und eingesammelt.
            if worker.is_finished() {
                let _ = worker.join();
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}
