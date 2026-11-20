//! Nonblocking input and observation dialogs on the existing stdio session.
use std::sync::{atomic::Ordering, mpsc::TryRecvError};
use std::time::Instant;

use super::{AdapterMessage, HostMessage, Session, StdioAdapter, StdioError};
use crate::adapters::{AdapterId, AdapterRegistry};
use crate::core::{Artifact, ArtifactIdGenerator, Clock, SchemaRegistry};
use crate::input::{InputMessage, InputService};
use crate::store::SqliteArtifactStore;
use crate::world::observation::{ObservationMessage, ObservationService, RecordedObservation};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressKind {
    Input,
    Observation,
}

pub enum IngressReply {
    Idle,
    Message(Box<IngressMessage>),
    Acknowledged,
}

enum IngressEvent {
    Input(InputMessage),
    Observation(ObservationMessage),
}

pub struct IngressMessage {
    adapter: AdapterId,
    message_id: String,
    event: IngressEvent,
}

/// Created only by the Core after its authorized, idempotent event commit.
pub struct IngressAcknowledgement {
    adapter: AdapterId,
    kind: IngressKind,
    message_id: String,
}

pub enum CommittedIngress {
    Input(Artifact),
    Observation(RecordedObservation),
}

impl IngressMessage {
    pub fn commit(
        self,
        registry: &AdapterRegistry,
        schemas: &SchemaRegistry,
        store: &SqliteArtifactStore,
        ids: &mut dyn ArtifactIdGenerator,
        clock: &dyn Clock,
    ) -> Result<(CommittedIngress, IngressAcknowledgement), StdioError> {
        let Self {
            adapter,
            message_id,
            event,
        } = self;
        let (kind, result) = match event {
            IngressEvent::Input(input) => (
                IngressKind::Input,
                CommittedIngress::Input(
                    InputService::new(registry, schemas, store, ids, clock)
                        .ingest_once(&adapter, &message_id, input)
                        .map_err(StdioError::Input)?,
                ),
            ),
            IngressEvent::Observation(observation) => (
                IngressKind::Observation,
                CommittedIngress::Observation(
                    ObservationService::new(registry, schemas, store, ids, clock)
                        .ingest_once(&adapter, &message_id, observation)
                        .map_err(StdioError::Observation)?,
                ),
            ),
        };
        Ok((
            result,
            IngressAcknowledgement {
                adapter,
                kind,
                message_id,
            },
        ))
    }
}

enum IngressExchange {
    Poll(IngressKind),
    Acknowledge(IngressKind, String),
}

pub(super) struct PendingIngress {
    exchange: IngressExchange,
    request_id: String,
    started: Instant,
}

impl StdioAdapter {
    pub fn start_ingress_poll(&self, kind: IngressKind) -> Result<(), StdioError> {
        self.start_ingress(IngressExchange::Poll(kind))
    }

    pub fn start_ingress_ack(&self, ack: IngressAcknowledgement) -> Result<(), StdioError> {
        if ack.adapter != self.manifest.adapter_id {
            return Err(StdioError::UnexpectedMessage);
        }
        self.start_ingress(IngressExchange::Acknowledge(ack.kind, ack.message_id))
    }

    fn start_ingress(&self, exchange: IngressExchange) -> Result<(), StdioError> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| StdioError::SessionPoisoned)?;
        if session.ingress.is_some() {
            return Err(StdioError::ExchangeInProgress);
        }
        let started = Instant::now();
        match session.send(exchange.message()) {
            Ok(request_id) => {
                session.ingress = Some(PendingIngress {
                    exchange,
                    request_id,
                    started,
                });
                Ok(())
            }
            Err(error) => {
                session.stop();
                Err(error)
            }
        }
    }

    /// Retire a failed or interrupted ingress session and reap its child.
    pub fn stop_ingress(&self) {
        self.abort();
        self.stop();
    }

    /// None means the session is still awaiting the reply to the pending dialog.
    pub fn collect_ingress(&self) -> Result<Option<IngressReply>, StdioError> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| StdioError::SessionPoisoned)?;
        let result = session.collect_ingress(&self.manifest.adapter_id);
        if result.is_err() {
            session.stop();
        }
        result
    }
}

impl Session {
    fn collect_ingress(&mut self, adapter: &AdapterId) -> Result<Option<IngressReply>, StdioError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(StdioError::Disconnected);
        }
        let pending = self.ingress.as_ref().ok_or(StdioError::NoIngressExchange)?;
        match self.responses.try_recv() {
            Ok((received, response)) => {
                let pending = self.ingress.take().expect("pending ingress dialog");
                if received.duration_since(pending.started) >= self.timeout {
                    return Err(StdioError::Timeout);
                }
                let reply =
                    Self::correlate(&pending.request_id, response.map_err(StdioError::Wire)?)?;
                pending.exchange.decode(reply, adapter).map(Some)
            }
            Err(TryRecvError::Empty) if pending.started.elapsed() < self.timeout => Ok(None),
            Err(TryRecvError::Empty) => Err(StdioError::Timeout),
            Err(TryRecvError::Disconnected) => Err(StdioError::Disconnected),
        }
    }
}

impl IngressExchange {
    fn message(&self) -> HostMessage {
        match self {
            Self::Poll(IngressKind::Input) => HostMessage::PollInput,
            Self::Poll(IngressKind::Observation) => HostMessage::Poll,
            Self::Acknowledge(IngressKind::Input, id) => HostMessage::AckInput {
                message_id: id.clone(),
            },
            Self::Acknowledge(IngressKind::Observation, id) => HostMessage::Ack {
                message_id: id.clone(),
            },
        }
    }

    fn decode(
        self,
        response: AdapterMessage,
        adapter: &AdapterId,
    ) -> Result<IngressReply, StdioError> {
        let message = |message_id, event| {
            IngressReply::Message(Box::new(IngressMessage {
                adapter: adapter.clone(),
                message_id,
                event,
            }))
        };
        match (self, response) {
            (Self::Poll(_), AdapterMessage::Idle) => Ok(IngressReply::Idle),
            (Self::Poll(IngressKind::Input), AdapterMessage::Input { message_id, input }) => {
                Ok(message(message_id, IngressEvent::Input(input)))
            }
            (
                Self::Poll(IngressKind::Observation),
                AdapterMessage::Observation {
                    message_id,
                    observation,
                },
            ) => Ok(message(message_id, IngressEvent::Observation(observation))),
            (
                Self::Acknowledge(IngressKind::Input, expected),
                AdapterMessage::InputAcknowledged { message_id },
            )
            | (
                Self::Acknowledge(IngressKind::Observation, expected),
                AdapterMessage::Acknowledged { message_id },
            ) if expected == message_id => Ok(IngressReply::Acknowledged),
            _ => Err(StdioError::UnexpectedMessage),
        }
    }
}
